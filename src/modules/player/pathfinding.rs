use std::collections::{HashMap, VecDeque};
use crate::modules::map::{Map, TileKind};

pub fn find_path(map: &Map, from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
    if from == to { return vec![]; }
    if map.get_tile(to.0, to.1) != TileKind::Floor { return vec![]; }

    let mut queue: VecDeque<(usize, usize)> = VecDeque::new();
    let mut came_from: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    queue.push_back(from);
    came_from.insert(from, from);

    while let Some(current) = queue.pop_front() {
        if current == to { break; }
        let (cx, cy) = current;
        let deltas: [(i32, i32); 8] = [
            (-1, 0), (1, 0), (0, -1), (0, 1),
            (-1, -1), (1, -1), (-1, 1), (1, 1),
        ];
        for (dx, dy) in deltas {
            let nx = cx as i32 + dx;
            let ny = cy as i32 + dy;
            if nx < 0 || ny < 0 { continue; }
            let (nx, ny) = (nx as usize, ny as usize);
            if nx >= map.width || ny >= map.height { continue; }
            if map.get_tile(nx, ny) != TileKind::Floor { continue; }
            // 대각선 이동 시 양쪽 인접 카디널 타일 중 하나라도 Wall이면 이동 불가 (코너 끼임 방지)
            if dx != 0 && dy != 0 {
                let ax = (cx as i32 + dx) as usize;
                let ay = cy;
                let bx = cx;
                let by = (cy as i32 + dy) as usize;
                if map.get_tile(ax, ay) != TileKind::Floor && map.get_tile(bx, by) != TileKind::Floor {
                    continue;
                }
            }
            let next = (nx, ny);
            if came_from.contains_key(&next) { continue; }
            came_from.insert(next, current);
            queue.push_back(next);
        }
    }

    if !came_from.contains_key(&to) { return vec![]; }

    let mut path = Vec::new();
    let mut current = to;
    while current != from {
        path.push(current);
        current = came_from[&current];
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_map(w: usize, h: usize, floors: &[(usize, usize)]) -> Map {
        let mut map = Map::new(w, h);
        for &(x, y) in floors {
            map.set_tile(x, y, TileKind::Floor);
        }
        map
    }

    #[test]
    fn find_path_straight_line() {
        let map = make_map(10, 10, &[(1,1),(2,1),(3,1),(4,1)]);
        let path = find_path(&map, (1,1), (4,1));
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), (4,1));
        for step in &path {
            assert_eq!(map.get_tile(step.0, step.1), TileKind::Floor);
        }
    }

    #[test]
    fn find_path_diagonal_shorter_than_cardinal() {
        // 대각선이 열려있으면 4스텝보다 짧은 경로로 도달해야 한다
        let mut floors = Vec::new();
        for x in 0..5usize { for y in 0..5usize { floors.push((x, y)); } }
        let map = make_map(5, 5, &floors);
        let path = find_path(&map, (0,0), (3,3));
        // 4방향만이면 6스텝, 대각선이면 3스텝
        assert!(path.len() <= 3, "대각선 경로는 3스텝 이하여야 한다. 실제: {}", path.len());
    }

    #[test]
    fn find_path_same_tile_returns_empty() {
        let map = make_map(10, 10, &[(1,1)]);
        let path = find_path(&map, (1,1), (1,1));
        assert!(path.is_empty());
    }

    #[test]
    fn find_path_wall_target_returns_empty() {
        let map = make_map(10, 10, &[(1,1),(2,1)]);
        let path = find_path(&map, (1,1), (5,5));
        assert!(path.is_empty());
    }

    #[test]
    fn find_path_unreachable_returns_empty() {
        let map = make_map(10, 10, &[(1,1),(5,5)]);
        let path = find_path(&map, (1,1), (5,5));
        assert!(path.is_empty(), "연결되지 않은 경우 빈 경로를 반환해야 한다");
    }

    #[test]
    fn find_path_goes_around_obstacle() {
        // FFF  row 0: (0,0)(1,0)(2,0)
        // F.F  row 1: (0,1) wall(1,1) (2,1)
        // FFF  row 2: (0,2)(1,2)(2,2)
        let floors = &[(0,0),(1,0),(2,0),(0,1),(2,1),(0,2),(1,2),(2,2)];
        let map = make_map(3, 3, floors);
        let path = find_path(&map, (0,1), (2,1));
        assert!(!path.is_empty(), "우회 경로가 존재해야 한다");
        assert_eq!(*path.last().unwrap(), (2,1));
        for step in &path {
            assert_eq!(map.get_tile(step.0, step.1), TileKind::Floor, "경로는 Floor 위에만 있어야 한다");
        }
    }

    #[test]
    fn find_path_result_excludes_start_includes_end() {
        let map = make_map(10, 10, &[(0,0),(1,0),(2,0)]);
        let path = find_path(&map, (0,0), (2,0));
        assert!(!path.contains(&(0,0)), "시작 타일은 경로에 포함되지 않아야 한다");
        assert_eq!(*path.last().unwrap(), (2,0), "끝 타일은 경로에 포함돼야 한다");
    }
}
