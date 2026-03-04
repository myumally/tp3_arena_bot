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
    sync::{
        mpsc::{self, SendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle, Thread},
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

pub struct MineRequestBatch {
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
    requests: Arc<Mutex<VecDeque<MineRequestBatch>>>,
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
    ///   - pow::pow_search() prend un start_nonce et un batch_size.
    ///     Utilisez rand::random::<u64>() comme start_nonce pour que chaque
    ///     appel explore une zone différente.
    ///   - Batch size recommandé : 100_000
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
        let requests = Arc::new(Mutex::new(VecDeque::<MineRequestBatch>::new()));

        // Create threads pool
        let mut pool = vec![];
        for _ in 0..n {
            let thread_results_tx = Arc::clone(&results_tx);
            let thread_results_rx = Arc::clone(&results_rx);
            let thread_requests = Arc::clone(&requests);

            pool.push(thread::spawn(move || {
                loop {
                    let target = {
                        let requests = thread_requests.lock().unwrap();

                        // If empty, wait for more
                        if requests.len() == 0 {
                            thread::sleep(Duration::from_millis(100));
                            continue;
                        }

                        // Get the element
                        let mut target = requests.front_mut().unwrap();
                        target.remaining_priority -= 1;
                        target.start_nonce += 100_000;

                        // Refill if empty
                        if target.remaining_priority == 0 {
                            requests.pop_front();
                            requests.push_back(*target);
                        }

                        target
                    };

                    let MineRequest {
                        seed,
                        tick,
                        resource_id,
                        agent_id,
                        target_bits,
                    } = target.mine_request;

                    // If we find an hash
                    if let Some(nonce) = pow_search(
                        &seed,
                        tick,
                        resource_id,
                        agent_id,
                        target_bits,
                        target.start_nonce,
                        100_000,
                    ){
                        // Add it to results
                        thread_results_tx.lock().unwrap().send(MineResult {tick, resource_id, nonce}).unwrap();

                        // Iterate on queue to delete if same id
                        let mut requests = thread_requests.lock().unwrap(); 
                        for i in 0..requests.len() {
                            if requests.get(i).unwrap().mine_request.resource_id == target.mine_request.resource_id {
                                requests.remove(i);
                                break;
                            }
                        }
                    }
                    
                }
            }));
        }

        MinerPool {
            requests: channel_request_tx,
            results: channel_result_rx,
            requests_rx: requests_rx,
            results_tx: results_tx,
            pool: pool,
        }
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
