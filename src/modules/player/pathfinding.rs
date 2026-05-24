use std::collections::{HashMap, VecDeque};
use crate::modules::map::Map;

pub fn find_path(map: &Map, from: (usize, usize), to: (usize, usize)) -> Vec<(usize, usize)> {
    if from == to { return vec![]; }
    if !map.get_tile(to.0, to.1).is_walkable() { return vec![]; }

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
            if !map.get_tile(nx, ny).is_walkable() { continue; }
            // 대각선 이동 시 양쪽 인접 카디널 타일 중 하나라도 통과 불가면 이동 불가 (코너 끼임 방지)
            if dx != 0 && dy != 0 {
                let ax = (cx as i32 + dx) as usize;
                let ay = cy;
                let bx = cx;
                let by = (cy as i32 + dy) as usize;
                if !map.get_tile(ax, ay).is_walkable() && !map.get_tile(bx, by).is_walkable() {
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
    use crate::modules::map::TileKind;

    fn make_map(w: usize, h: usize, floors: &[(usize, usize)]) -> Map {
        let mut map = Map::new(w, h);
        for &(x, y) in floors {
            map.set_tile(x, y, TileKind::Floor);
        }
        map
    }

    #[test]
    fn 일직선_통로에서_경로를_찾는다() {
        let map = make_map(10, 10, &[(1,1),(2,1),(3,1),(4,1)]);
        let path = find_path(&map, (1,1), (4,1));
        assert!(!path.is_empty());
        assert_eq!(*path.last().unwrap(), (4,1));
        for step in &path {
            assert_eq!(map.get_tile(step.0, step.1), TileKind::Floor);
        }
    }

    #[test]
    fn 대각선이_열려있으면_사방향보다_짧은_경로를_찾는다() {
        // 대각선이 열려있으면 4스텝보다 짧은 경로로 도달해야 한다
        let mut floors = Vec::new();
        for x in 0..5usize { for y in 0..5usize { floors.push((x, y)); } }
        let map = make_map(5, 5, &floors);
        let path = find_path(&map, (0,0), (3,3));
        // 4방향만이면 6스텝, 대각선이면 3스텝
        assert!(path.len() <= 3, "대각선 경로는 3스텝 이하여야 한다. 실제: {}", path.len());
    }

    #[test]
    fn 출발지와_목적지가_같으면_빈_경로를_반환한다() {
        let map = make_map(10, 10, &[(1,1)]);
        let path = find_path(&map, (1,1), (1,1));
        assert!(path.is_empty());
    }

    #[test]
    fn 목적지가_벽이면_빈_경로를_반환한다() {
        let map = make_map(10, 10, &[(1,1),(2,1)]);
        let path = find_path(&map, (1,1), (5,5));
        assert!(path.is_empty());
    }

    #[test]
    fn 도달_불가능하면_빈_경로를_반환한다() {
        let map = make_map(10, 10, &[(1,1),(5,5)]);
        let path = find_path(&map, (1,1), (5,5));
        assert!(path.is_empty(), "연결되지 않은 경우 빈 경로를 반환해야 한다");
    }

    #[test]
    fn 장애물이_있으면_우회_경로를_찾는다() {
        // FFF  0행: (0,0)(1,0)(2,0)
        // F.F  1행: (0,1) 벽(1,1) (2,1)
        // FFF  2행: (0,2)(1,2)(2,2)
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
    fn 경로는_출발지를_제외하고_목적지를_포함한다() {
        let map = make_map(10, 10, &[(0,0),(1,0),(2,0)]);
        let path = find_path(&map, (0,0), (2,0));
        assert!(!path.contains(&(0,0)), "시작 타일은 경로에 포함되지 않아야 한다");
        assert_eq!(*path.last().unwrap(), (2,0), "끝 타일은 경로에 포함돼야 한다");
    }

    #[test]
    fn 경로탐색은_물타일을_지나가지_않는다() {
        // 일직선 통로 가운데를 Water 로 막으면 경로가 끊겨 빈 경로를 반환한다.
        let mut map = make_map(10, 3, &[(1,1),(2,1),(3,1),(4,1),(5,1)]);
        map.set_tile(3, 1, TileKind::Water);
        let path = find_path(&map, (1,1), (5,1));
        assert!(path.is_empty(), "물 타일이 가로막으면 경로가 없어야 한다");
    }

    #[test]
    fn 경로탐색은_모래타일을_통과한다() {
        // 통로 가운데가 Sand 면 통과 가능하므로 경로가 존재한다.
        let mut map = make_map(10, 3, &[(1,1),(2,1),(3,1),(4,1),(5,1)]);
        map.set_tile(3, 1, TileKind::Sand);
        let path = find_path(&map, (1,1), (5,1));
        assert!(!path.is_empty(), "모래 타일은 통과 가능해야 한다");
        assert!(path.contains(&(3,1)), "경로가 모래 타일을 지나야 한다");
        assert_eq!(*path.last().unwrap(), (5,1));
    }

    #[test]
    fn 경로탐색_목적지가_물이면_빈경로다() {
        let mut map = make_map(10, 3, &[(1,1),(2,1),(3,1)]);
        map.set_tile(3, 1, TileKind::Water);
        let path = find_path(&map, (1,1), (3,1));
        assert!(path.is_empty(), "목적지가 물이면 경로가 없어야 한다");
    }

    #[test]
    fn 대각선의_한쪽_카디널만_막혀있으면_틈으로_지나간다() {
        // (0,0)->(1,1) 대각 이동에서 (1,0) 만 벽이고 (0,1) 은 바닥이면
        // 코너 끼임이 아니므로 대각 이동이 허용돼 1스텝 경로가 나온다.
        //   (0,0)F  (1,0)W
        //   (0,1)F  (1,1)F
        let map = make_map(3, 3, &[(0,0),(0,1),(1,1)]);
        let path = find_path(&map, (0,0), (1,1));
        assert!(!path.is_empty(), "한쪽만 막힌 대각선은 통과 가능해야 한다");
        assert_eq!(path, vec![(1,1)], "한 번의 대각 이동으로 도달해야 한다");
    }

    #[test]
    fn 대각선_양쪽_카디널이_모두_막히면_코너로_끼지_않는다() {
        // (0,0)->(1,1) 에서 (1,0)/(0,1) 둘 다 벽이면 대각 이동 불가.
        // 다른 통로도 없으므로 빈 경로.
        //   (0,0)F  (1,0)W
        //   (0,1)W  (1,1)F
        let map = make_map(3, 3, &[(0,0),(1,1)]);
        let path = find_path(&map, (0,0), (1,1));
        assert!(path.is_empty(), "양쪽이 막힌 대각선은 코너 끼임으로 막혀야 한다");
    }

    #[test]
    fn 경로탐색은_맵_경계_밖으로_확장하지_않는다() {
        // 좌상단 (0,0) 에서 출발하면 좌/상 방향 이웃은 음수 좌표라
        // nx<0||ny<0 분기로 걸러지고, 우/하 방향으로만 탐색해 도달한다.
        let map = make_map(3, 3, &[(0,0),(1,0),(2,0),(0,1),(0,2)]);
        let path = find_path(&map, (0,0), (2,0));
        assert!(!path.is_empty(), "경계 안에서는 경로가 존재해야 한다");
        assert_eq!(*path.last().unwrap(), (2,0));
    }
}
