// ─── Pathfinding BFS ─────────────────────────────────────────────────────────
//
// Convertit l'état du jeu en grille 100x100 et calcule les distances
// depuis un point de départ via BFS avec double buffer.
//
// ─────────────────────────────────────────────────────────────────────────────

use uuid::Uuid;

use crate::state::GameState;

/// Taille fixe de la grille.
pub const GRID_SIZE: usize = 100;

/// Valeur pour les obstacles (ne jamais traverser).
const OBSTACLE: i32 = -1;
const INFINITY: i32 = i32::MAX;
const RESSOURCE: i32 = i32::MAX - 1;

/// Voisins en 4-connexité (haut, bas, gauche, droite).
const NEIGHBORS: [(i32, i32); 4] = [(0, -1), (0, 1), (-1, 0), (1, 0)];

/// Convertit l'état du jeu en grille 100x100 et exécute un BFS depuis le point
/// de départ pour calculer les distances jusqu'à toutes les cases accessibles.
///
/// - Obstacles : -1 (jamais traversés)
/// - Par défaut : +inf (i32::MAX)
/// - Cases accessibles : distance depuis le point de départ
///
/// # Returns
///
/// Une matrice 100x100 où `dist[y][x]` contient la distance (ou -1 si obstacle,
/// ou INFINITY si inaccessible).
pub fn bfs_distance_grid(
    state: &GameState,
    start_x: u16,
    start_y: u16,
    stop_when_ressource: bool,
) -> (Vec<Vec<i32>>, Option<(u16, u16)>) {
    // Initialiser la grille : infini partout
    let (width, height) = state.map_size;
    let mut distances = vec![vec![INFINITY; width as usize]; height as usize];

    // Marquer les obstacles à -1
    for &(ox, oy) in &state.obstacles {
        distances[oy as usize][ox as usize] = OBSTACLE;
    }

    // Marquer les ressources
    if stop_when_ressource {
        for resource in &state.pow_challenge {
            distances[resource.y as usize][resource.x as usize] = RESSOURCE;
        }
    }

    // Marquer les joueurs à -1
    for agent in &state.agents {
        distances[agent.y as usize][agent.x as usize] = OBSTACLE;
    }

    // Point de départ en coordonnées grille
    let (sx, sy) = (start_x as usize, start_y as usize);
    let mut current_dist: i32 = 0;
    distances[sy][sx] = current_dist;
    let mut buffer_a: Vec<(usize, usize)> = vec![(sx, sy)];
    let mut buffer_b: Vec<(usize, usize)> = Vec::new();

    loop {
        current_dist += 1;
        while let Some((cx, cy)) = buffer_a.pop() {
            // Pour tout voisins
            for &(dx, dy) in &NEIGHBORS {
                let nx = cx as i32 + dx;
                let ny = cy as i32 + dy;

                // Check map bordertick
                if nx < 0 || nx >= width as i32 || ny < 0 || ny >= height as i32 {
                    continue;
                }

                // Check if ressource
                let dist = distances[ny as usize][nx as usize];
                if dist == RESSOURCE {
                    return (distances, Some((nx as u16, ny as u16)));
                }

                // Check if better distance
                if current_dist < dist {
                    distances[ny as usize][nx as usize] = current_dist;
                    buffer_b.push((nx as usize, ny as usize));
                }
            }
        }

        // Swap buffers untils no more elements
        std::mem::swap(&mut buffer_a, &mut buffer_b);
        if buffer_a.is_empty() {
            break;
        }
    }
    (distances, None)
}

pub fn find_closest_resource(state: &GameState) -> Option<((i32, i32), Uuid)> {
    // Calculate distances
    let (_distances, some_ressource) =
        bfs_distance_grid(state, state.position.0, state.position.1, true);

    if some_ressource.is_none() {
        return None;
    }
    let best_resource = some_ressource.unwrap();
    let best_resource_uuid = state
        .resources
        .iter()
        .find(|r| r.x == best_resource.0 && r.y == best_resource.1)
        .unwrap()
        .resource_id;

    // Run BFS from ressource and take direction reducing distance
    let (dist, _) = bfs_distance_grid(state, best_resource.0, best_resource.1, false);
    let mut best_dist = dist[state.position.1 as usize][state.position.0 as usize];
    let mut best_move: Option<(i32, i32)> = None;

    for (dx, dy) in NEIGHBORS {
        let nx = state.position.0 as i32 + dx;
        let ny = state.position.1 as i32 + dy;
        if nx < 0 || nx >= state.map_size.0 as i32 || ny < 0 || ny >= state.map_size.1 as i32 {
            continue;
        }

        if dist[ny as usize][nx as usize] < best_dist {
            best_dist = dist[ny as usize][nx as usize];
            best_move = Some((dx, dy));
        }
    }

    if best_move.is_none() {
        println!("Warning: Best ressource is accessible but can't find direction to the ressource");
        return None;
    }

    // Return best move and resource uuid
    Some((best_move.unwrap(), best_resource_uuid))
}
