use rand::prelude::*;
use crate::modules::map::{Map, MapTile, Rect};
use super::super::MapGenerator;
use super::carve_corridor;

pub struct PrefabGenerator;

impl MapGenerator for PrefabGenerator {
    fn generate(&self, width: usize, height: usize) -> Map {
        let mut map = Map::new(width, height);
        let mut rng = thread_rng();
        let mut rooms: Vec<Rect> = Vec::new();

        for _ in 0..300 {
            if rooms.len() >= 12 { break; }
            let template = rng.gen_range(0..TEMPLATES.len());
            let (tw, th, layout) = TEMPLATES[template];
            let x = rng.gen_range(2..width.saturating_sub(tw + 2));
            let y = rng.gen_range(2..height.saturating_sub(th + 2));

            let r = Rect::new(x, y, tw, th);
            if rooms.iter().any(|other| {
                r.x1 < other.x2 + 2 && r.x2 + 2 > other.x1
                    && r.y1 < other.y2 + 2 && r.y2 + 2 > other.y1
            }) {
                continue;
            }

            stamp_template(&mut map, x, y, tw, th, layout);
            rooms.push(r);
        }

        for i in 0..rooms.len().saturating_sub(1) {
            let (x1, y1) = rooms[i].center();
            let (x2, y2) = rooms[i + 1].center();
            carve_corridor(&mut map, x1, y1, x2, y2);
        }

        map.rooms = rooms;
        map
    }
    fn name(&self) -> &str { "prefab" }
}

fn stamp_template(map: &mut Map, ox: usize, oy: usize, w: usize, h: usize, layout: &[u8]) {
    let row_len = w + 1; // 줄바꿈 포함
    for row in 0..h {
        for col in 0..w {
            let idx = row * row_len + col;
            if idx >= layout.len() { break; }
            let tile = match layout[idx] {
                b'#' => MapTile::Wall,
                b'.' => MapTile::Floor,
                _ => continue,
            };
            let tx = ox + col;
            let ty = oy + row;
            if tx < map.width && ty < map.height {
                map.set_tile(tx, ty, tile);
            }
        }
    }
}

// (width, height, ascii_layout)
const TEMPLATES: &[(usize, usize, &[u8])] = &[
    (8, 5, b"########\n\
             #......#\n\
             #......#\n\
             #......#\n\
             ####.###"),
    (10, 6, b"##########\n\
              #........#\n\
              #....#...#\n\
              #....#...#\n\
              #........#\n\
              #####.####"),
    (12, 7, b"############\n\
              #..........#\n\
              #..######..#\n\
              #..#....#..#\n\
              #..######..#\n\
              #..........#\n\
              #####..#####"),
    (8, 8, b"########\n\
             #......#\n\
             #.####.#\n\
             #.#..#.#\n\
             #.#..#.#\n\
             #.####.#\n\
             #......#\n\
             ###.####"),
    (14, 5, b"##############\n\
              #............#\n\
              #....####....#\n\
              #............#\n\
              #######.######"),
];
