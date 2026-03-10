// Le squelette contient du code fourni pas encore utilisé — c'est normal.
#![allow(dead_code)]

// ─────────────────────────────────────────────────────────────────────
//  Objectif : implémenter la boucle principale du bot.
//
//  Étapes suggérées :
//
//  1. Créer l'état partagé (state::SharedState)
//     let shared_state = state::SharedState::new(agent_id);
//
//  2. Créer le pool de mineurs (miner::MinerPool)
//     let miner_pool = miner::MinerPool::new(NUM_MINERS);
//
//  3. Créer la stratégie de déplacement
//     let strategy: Box<dyn strategy::Strategy> = Box::new(strategy::NearestResourceStrategy);
//
//  4. Séparer la WebSocket en lecture/écriture
//     Utiliser ws.into_inner() pour récupérer le TcpStream puis séparer
//     via std::io::Read/Write. Sinon, approche plus simple ci-dessous :
//
//  ─── Approche simplifiée (recommandée) ─────────────────────────────
//
//  Utiliser ws.read() dans un thread dédié qui :
//    a) parse les ServerMsg
//    b) met à jour le SharedState
//    c) envoie les PowChallenge au MinerPool via un channel
//
//  Le thread principal :
//    a) vérifie si le MinerPool a trouvé une solution → envoie PowSubmit
//    b) consulte la stratégie pour décider du prochain mouvement → envoie Move
//    c) dort un court instant (ex: 50ms) pour ne pas surcharger
//
//  Contrainte : la WebSocket (tungstenite) n'est pas Send si on utilise
//  la version par défaut. Vous devrez garder toutes les écritures WS
//  dans le thread principal, et utiliser des channels pour communiquer
//  depuis le thread lecteur.
// ─────────────────────────────────────────────────────────────────────

mod miner;
mod pathfinding;
mod pow;
mod protocol;
mod state;
mod strategy;

// Ces imports seront utilisés dans votre implémentation.
use std::io::ErrorKind;
#[allow(unused_imports)]
use std::sync::{Arc, Mutex};
#[allow(unused_imports)]
use std::thread;
#[allow(unused_imports)]
use std::time::Duration;

use tungstenite::stream::MaybeTlsStream;

use tungstenite::{connect, Message};
#[allow(unused_imports)]
use uuid::Uuid;

use pathfinding::{find_closest_resource, find_direction_towards};
use protocol::{ClientMsg, ServerMsg};

use crate::miner::MineResult;

// ─── Configuration ──────────────────────────────────────────────────────────
const SERVER_URL: &str = "ws://127.0.0.1:4004/ws";
const TEAM_NAME: &str = "mon_equipe";
const AGENT_NAME: &str = "bot_1";
const NUM_MINERS: usize = 4;

fn main() {
    println!("[*] Connexion à {SERVER_URL}...");
    let (mut ws, _response) = connect(SERVER_URL).expect("impossible de se connecter au serveur");
    println!("[*] Connecté !");

    // ── Attendre le Hello ────────────────────────────────────────────────
    #[allow(unused_variables)] // Vous utiliserez agent_id dans votre implémentation.
    let agent_id: Uuid = match read_server_msg(&mut ws) {
        Some(ServerMsg::Hello { agent_id, tick_ms }) => {
            println!("[*] Hello reçu : agent_id={agent_id}, tick={tick_ms}ms");
            agent_id
        }
        other => panic!("premier message inattendu : {other:?}"),
    };

    // ── S'enregistrer ────────────────────────────────────────────────────
    send_client_msg(
        &mut ws,
        &ClientMsg::Register {
            team: TEAM_NAME.into(),
            name: AGENT_NAME.into(),
        },
    );
    println!("[*] Enregistré en tant que {AGENT_NAME} (équipe {TEAM_NAME})");

    // ─── Initialisation ──────────────────────────────────────────────────
    let shared_state = state::new_shared_state(agent_id);
    let mut miner_pool = miner::MinerPool::new();
    miner_pool.populate(NUM_MINERS);

    // Configurer un timeout de lecture pour éviter le blocage (ws:// sans TLS)
    if let MaybeTlsStream::Plain(ref mut tcp) = *ws.get_mut() {
        tcp.set_read_timeout(Some(Duration::from_millis(50)))
            .expect("set_read_timeout");
    }
    let mut target: Option<miner::MineResult> = None;

    // ─── Boucle principale ──────────────────────────────────────────────────
    loop {
        // On lit les messages du serveur
        let msg = read_server_msg(&mut ws);
        if let Some(msg) = msg {
            shared_state.lock().unwrap().update(&msg);

            if let ServerMsg::Win { team } = msg {
                println!("[!] Win reçu : {team}");
                break;
            }
        }
        let state = shared_state.lock().unwrap();

        // On vérifi que le terrain est bien initialisé
        if state.map_size == (0, 0) {
            continue;
        }

        // ─── Si résultat trouvé ──────────────────────────────────────────────────
        if target.is_none() {
            target = miner_pool.try_recv();
        }
        if let Some(result) = target.clone() {
            // On récupère ses coordonnées
            let resource_pos = state
                .pow_challenge
                .iter()
                .find(|c| c.resource_id == result.resource_id)
                .map(|c| (c.x, c.y))
                .or_else(|| {
                    state
                        .resources
                        .iter()
                        .find(|r| r.resource_id == result.resource_id)
                        .map(|r| (r.x, r.y))
                });

            // On se déplace si elle existe
            if let Some((rx, ry)) = resource_pos {
                let manhattan = (state.position.0 as i32 - rx as i32).abs()
                    + (state.position.1 as i32 - ry as i32).abs();
                if manhattan == 1 {
                    send_client_msg(
                        &mut ws,
                        &ClientMsg::PowSubmit {
                            tick: result.tick,
                            resource_id: result.resource_id,
                            nonce: result.nonce,
                        },
                    );
                } else if let Some((dx, dy)) = find_direction_towards(&state, rx, ry) {
                    send_client_msg(
                        &mut ws,
                        &ClientMsg::Move {
                            dx: dx as i8,
                            dy: dy as i8,
                        },
                    );
                }

            // Sinon on remet la target à None
            } else {
                target = None;
            }

        // ─── Si pas de résultat trouvé ──────────────────────────────────────────────────
        } else {
            // On se déplace vers la ressource la plus proche
            if let Some((rx, ry, resource_id)) = find_closest_resource(&state) {
                // Try to find associated challenge
                if miner_pool.current_resource_id() != Some(resource_id) {
                    if let Some(challenge) = state
                        .pow_challenge
                        .iter()
                        .find(|c| c.resource_id == resource_id)
                    {
                        miner_pool.submit(challenge.clone(), agent_id);
                    }
                }

                // Got to this direction
                if let Some((dx, dy)) = find_direction_towards(&state, rx, ry) {
                    send_client_msg(
                        &mut ws,
                        &ClientMsg::Move {
                            dx: dx as i8,
                            dy: dy as i8,
                        },
                    );
                }
            }

            thread::sleep(Duration::from_millis(50));
        }

        drop(state);
    }

    println!("[*] Boucle principale terminée.");
}

// ─── Fonctions utilitaires (fournies) ───────────────────────────────────────

type WsStream = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

/// Lit un message du serveur et le désérialise.
/// Retourne `None` en cas de timeout (lecture non bloquante) — sans log.
fn read_server_msg(ws: &mut WsStream) -> Option<ServerMsg> {
    match ws.read() {
        Ok(Message::Text(text)) => match serde_json::from_str(&text) {
            Ok(msg) => {
                println!("Message reçu: {:?}", msg);
                Some(msg)
            }
            Err(e) => {
                let preview: String = text.chars().take(200).collect();
                eprintln!(
                    "[!] Message non décodé : {} — JSON (extrait) : {:?}",
                    e, preview
                );
                None
            }
        },
        Ok(_) => None,
        Err(e) => {
            // Ne pas afficher pour timeout/would_block (polling normal)
            let is_timeout = matches!(&e, tungstenite::Error::Io(io) if io.kind() == ErrorKind::TimedOut || io.kind() == ErrorKind::WouldBlock);
            if !is_timeout {
                eprintln!("[!] Erreur WS lecture : {e}");
            }
            None
        }
    }
}

/// Sérialise et envoie un message au serveur.
fn send_client_msg(ws: &mut WsStream, msg: &ClientMsg) {
    let json = serde_json::to_string(msg).expect("sérialisation échouée");
    ws.send(Message::Text(json.into()))
        .expect("envoi WS échoué");
}
