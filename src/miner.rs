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

use std::{
    collections::VecDeque,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
    time::Duration,
};
use uuid::Uuid;

use crate::pow::pow_search;

/// Requête de minage envoyée aux threads mineurs.
#[derive(Debug, Clone)]
pub struct MineRequest {
    pub seed: String,
    pub tick: u64,
    pub resource_id: Uuid,
    pub agent_id: Uuid,
    pub target_bits: u8,
}

pub struct MineRequesttarget {
    pub mine_request: MineRequest,
    pub priority: usize,
    pub remaining_priority: usize,
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
    requests: Arc<Mutex<VecDeque<MineRequesttarget>>>,
    pool: Vec<JoinHandle<()>>,
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
    pub fn new(n: usize) -> Self {
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
        let requests = Arc::new(Mutex::new(VecDeque::<MineRequesttarget>::new()));
        let mut pool = vec![];

        MinerPool {
            results_rx: results_rx,
            results_tx: results_tx,
            requests: requests,
            pool: pool,
        }
    }

    fn pop_target(&self) -> Option<MineRequesttarget> {
        // Acces requests queue
        let mut reqs = self.requests.lock().unwrap();
        if reqs.is_empty() {
            return None;
        }
    
        // Extract target
        let front = reqs.front_mut().unwrap();
        let target = front.clone();

        // Update 
        front.remaining_priority = front.remaining_priority.saturating_sub(1);
        front.start_nonce += 100_000;

        // Refill if needed
        if front.remaining_priority == 0 {
            reqs.pop_front();
            front.remaining_priority = front.priority;
            reqs.push_back(*front);
        }

        Some(target)
    }

    pub fn populate(&mut self, n: usize) {
        for _ in 0..n {
            let thread_results_tx = Arc::clone(&self.results_tx);
            let thread_requests = Arc::clone(&self.requests);

            self.pool.push(thread::spawn(move || {
                loop {
                    // Get target, or wait
                    let target = self.pop_target();
                    if target.is_none() {
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    };


                    // Search for nonce 
                    let target = target.unwrap();
                    let mine_request = target.mine_request;
                    if let Some(nonce) = pow_search(&mine_request, target.start_nonce, 100_000) {
                        thread_results_tx
                            .lock()
                            .unwrap()
                            .send(MineResult {
                                tick: mine_request.tick,
                                resource_id: mine_request.resource_id,
                                nonce,
                            })
                            .unwrap();

                        self.remove_ressource(mine_request.resource_id);
                    }
                }
            }));
        }
    }





    pub fn remove_ressource(&self, resource_id: Uuid) {
        &self.requests.lock()
            .unwrap()
        .retain(|r| r.mine_request.resource_id != resource_id);;
    }

    /// Envoie un challenge de minage au pool.
    pub fn submit(&self, request: MineRequest) {
        match self.requests.send(request) {
            Err(send_error) => println!("Failed to add Minerequest: {:?}", send_error),
            Ok(_) => println!("Minerequest added successfully"),
        };
    }

    /// Tente de récupérer un résultat sans bloquer.
    pub fn try_recv(&self) -> Option<MineResult> {
        match self.results.try_recv() {
            Ok(result) => Some(result),
            Err(recv_error) => {
                println!("Failed to receive Mineresult: {:?}", recv_error);
                None
            }
        }
    }
}
