// ─── Partie 2 : Pool de mineurs ──────────────────────────────────────────────
//
// Objectif : créer un pool de N threads qui cherchent des nonces en parallèle.
//
// Concepts exercés : thread::spawn, mpsc::channel, Arc, move closures.
//
// Architecture :
//
//   Thread principal                        Threads mineurs (x N)
//        |                                        |
//        |── mpsc::Sender<MineRequest> ──────────>|  (challenges à résoudre)
//        |                                        |
//        |<── mpsc::Sender<MineResult> ──────────>|  (solutions trouvées)
//        |                                        |
//
// Chaque thread mineur :
//   1. Attend un MineRequest sur son channel
//   2. Appelle pow::pow_search() avec un start_nonce différent
//   3. Si un nonce est trouvé, envoie un MineResult
//
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use uuid::Uuid;
const BATCH_SIZE: u64 = 100_000;

use crate::pow::pow_search;
use crate::protocol::PowChallengeStruct;

/// Requête de minage envoyée aux threads mineurs.
#[derive(Debug, Clone)]
pub struct MineRequest {
    pub seed: String,
    pub tick: u64,
    pub resource_id: Uuid,
    pub agent_id: Uuid,
    pub target_bits: u8,
}

#[derive(Clone)]
pub struct MineRequestTarget {
    pub mine_request: MineRequest,
    pub start_nonce: u64,
}

/// Résultat renvoyé par un mineur quand il trouve un nonce valide.
#[derive(Debug, Clone)]
pub struct MineResult {
    pub tick: u64,
    pub resource_id: Uuid,
    pub nonce: u64,
}

// FAIT: Définir la structure MinerPool.
//
// Elle doit contenir :
//   - Le Sender pour envoyer des MineRequest aux threads
//   - Le Receiver pour récupérer les MineResult
//
// Indice : les types sont :
//   std::sync::mpsc::Sender<MineRequest>
//   std::sync::mpsc::Receiver<MineResult>
pub struct MinerPool {
    results_rx: Arc<Mutex<mpsc::Receiver<MineResult>>>,
    results_tx: Arc<Mutex<mpsc::Sender<MineResult>>>,
    target_arc: Arc<Mutex<Option<MineRequestTarget>>>,
    pool: Vec<JoinHandle<()>>,
}

/// Envoie un MineResult et remet la cible à None si elle correspond au résultat trouvé.
fn send_result(
    results_tx: &Arc<Mutex<mpsc::Sender<MineResult>>>,
    target_arc: &Arc<Mutex<Option<MineRequestTarget>>>,
    result: MineResult,
) {
    let resource_id = result.resource_id;
    if let Ok(tx) = results_tx.lock() {
        if let Err(e) = tx.send(result.clone()) {
            eprintln!("Error sending MineResult: {}", e);
        }
    }
    let mut guard = target_arc.lock().unwrap();
    if let Some(ref target) = *guard {
        if target.mine_request.resource_id == resource_id {
            *guard = None;
            println!("Result sent to the pool: {:?}, set target to None", result);
        }
    }
}

fn remove_ressource(target_arc: &Arc<Mutex<Option<MineRequestTarget>>>, resource_id: Uuid) {
    let mut guard = target_arc.lock().unwrap();
    if let Some(ref target) = *guard {
        if target.mine_request.resource_id == resource_id {
            *guard = None;
        }
    }
}

/// Renvoie une copie de la cible courante et incrémente start_nonce pour le prochain.
fn ask_task(target_arc: &Arc<Mutex<Option<MineRequestTarget>>>) -> Option<MineRequestTarget> {
    let mut guard = target_arc.lock().unwrap();
    if let Some(ref mut target) = *guard {
        let copy = target.clone();
        target.start_nonce += BATCH_SIZE;
        Some(copy)
    } else {
        None
    }
}

impl MinerPool {
    /// Crée un pool de `n` threads mineurs.
    ///
    /// Chaque thread :
    ///   1. Possède un Receiver<MineRequest> (partagé via Arc<Mutex<>>)
    ///   2. Possède un Sender<MineResult> (cloné pour chaque thread)
    ///   3. Boucle : recv() → pow_search() → send() si trouvé
    ///
    /// Indices :
    ///   - Un seul Receiver existe par channel. Pour le partager entre N threads,
    ///     il faut le wrapper dans Arc<Mutex<Receiver<MineRequest>>>.
    ///   - Chaque thread clone le Arc pour accéder au Receiver.
    ///   - pow::pow_search() prend un start_nonce et un target_size.
    ///     Utilisez rand::random::<u64>() comme start_nonce pour que chaque
    ///     appel explore une zone différente.
    ///   - target size recommandé : 100_000
    ///
    pub fn new() -> Self {
        //
        // Créer les 2 channels :
        //   - (request_tx, request_rx) pour envoyer les challenges
        //   - (result_tx, result_rx) pour recevoir les solutions
        //
        // Wrapper request_rx dans Arc<Mutex<>>
        //
        // Pour chaque thread (0..n) :
        //   - Cloner le Arc<Mutex<Receiver<MineRequest>>>
        //   - Cloner le result_tx
        //   - thread::spawn(move || { ... boucle de minage ... })
        //
        // Retourner MinerPool { request_tx, result_rx }

        // Init channels
        let (channel_result_tx, channel_result_rx) = mpsc::channel::<MineResult>();
        let results_tx = Arc::new(Mutex::new(channel_result_tx));
        let results_rx = Arc::new(Mutex::new(channel_result_rx));
        let target_arc = Arc::new(Mutex::new(None));
        let pool = vec![];

        MinerPool {
            results_rx: results_rx,
            results_tx: results_tx,
            target_arc: target_arc,
            pool: pool,
        }
    }

    pub fn populate(&mut self, n: usize) {
        let target_arc = Arc::clone(&self.target_arc);
        let results_tx = Arc::clone(&self.results_tx);

        for _ in 0..n {
            let target_arc = Arc::clone(&target_arc);
            let results_tx = Arc::clone(&results_tx);
            self.pool.push(thread::spawn(move || {
                loop {
                    // Demande une tâche (copie + incrément pour le prochain)
                    let target = ask_task(&target_arc);
                    let Some(target) = target else {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    };

                    let mine_request = target.mine_request;
                    match pow_search(&mine_request, target.start_nonce, BATCH_SIZE) {
                        Some(nonce) => {
                            send_result(
                                &results_tx,
                                &target_arc,
                                MineResult {
                                    tick: mine_request.tick,
                                    resource_id: mine_request.resource_id,
                                    nonce,
                                },
                            );
                        }
                        None => {
                            // Pas trouvé : ask_task a déjà incrémenté start_nonce
                        }
                    }
                }
            }));
        }
    }

    /// Envoie un challenge de minage au pool.
    pub fn submit(&self, challenge: PowChallengeStruct, agent_id: Uuid) {
        let mut guard = self.target_arc.lock().unwrap();
        if let Some(ref target) = *guard {
            if target.mine_request.resource_id == challenge.resource_id {
                return; // Même ressource, pas de mise à jour
            }
        }
        println!("Submit challenge to the pool: {:?}", challenge);
        let mine_request = MineRequest {
            tick: challenge.tick,
            seed: challenge.seed,
            resource_id: challenge.resource_id,
            target_bits: challenge.target_bits,
            agent_id,
        };
        *guard = Some(MineRequestTarget {
            mine_request,
            start_nonce: 0,
        });
    }

    /// Retourne la ressource actuellement ciblée par le pool, ou None.
    pub fn current_resource_id(&self) -> Option<Uuid> {
        self.target_arc
            .lock()
            .unwrap()
            .as_ref()
            .map(|t| t.mine_request.resource_id)
    }

    /// Tente de récupérer un résultat sans bloquer.
    pub fn try_recv(&self) -> Option<MineResult> {
        let rx = self.results_rx.lock().unwrap();
        match rx.try_recv() {
            Ok(result) => Some(result),
            Err(_) => None,
        }
    }

    pub fn clear_requests(&self) {
        *self.target_arc.lock().unwrap() = None;
    }
}
