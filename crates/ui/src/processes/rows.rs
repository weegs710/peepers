use std::collections::{HashMap, HashSet};

use peepers_core::process::Process;

/// One line of the process table: a process and where it sits in the tree.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    pub process: &'a Process,
    pub depth: usize,
    pub children: usize,
    pub collapsed: bool,
}

pub fn flat(processes: &[Process], keep: impl Fn(&Process) -> bool) -> Vec<Row<'_>> {
    processes
        .iter()
        .filter(|process| keep(process))
        .map(|process| Row {
            process,
            depth: 0,
            children: 0,
            collapsed: false,
        })
        .collect()
}

/// Parents above their children, siblings in the order given. A process whose parent is not in
/// the list, or is itself, starts a tree. Processes that only reach each other through looping
/// parent links, which a pid reused mid-sample can produce, still get a place. The subtree of a
/// collapsed process is walked but not listed, so its members are not mistaken for strays.
pub fn tree<'a>(processes: &'a [Process], collapsed: &HashSet<u32>) -> Vec<Row<'a>> {
    let known: HashSet<u32> = processes.iter().map(|process| process.pid).collect();
    let mut children: HashMap<u32, Vec<&Process>> = HashMap::new();
    for process in processes
        .iter()
        .filter(|p| p.parent != p.pid && known.contains(&p.parent))
    {
        children.entry(process.parent).or_default().push(process);
    }

    let roots = processes
        .iter()
        .filter(|p| p.parent == p.pid || !known.contains(&p.parent));
    let mut rows = Vec::with_capacity(processes.len());
    let mut visited = HashSet::new();
    for seed in roots.chain(processes) {
        let mut stack = vec![(seed, 0, false)];
        while let Some((process, depth, hidden)) = stack.pop() {
            if !visited.insert(process.pid) {
                continue;
            }
            let below = children.get(&process.pid).map_or(&[][..], Vec::as_slice);
            let is_collapsed = collapsed.contains(&process.pid);
            if !hidden {
                rows.push(Row {
                    process,
                    depth,
                    children: below.len(),
                    collapsed: is_collapsed,
                });
            }
            stack.extend(
                below
                    .iter()
                    .rev()
                    .map(|&child| (child, depth + 1, hidden || is_collapsed)),
            );
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use peepers_core::process::State;

    use super::*;

    fn process(pid: u32, parent: u32) -> Process {
        Process {
            pid,
            parent,
            name: format!("p{pid}"),
            state: State::Sleeping,
            uid: 0,
            threads: 1,
            nice: 0,
            cpu: 0.0,
            memory: 0,
            io: None,
            started: Duration::ZERO,
        }
    }

    fn pids(rows: &[Row]) -> Vec<u32> {
        rows.iter().map(|row| row.process.pid).collect()
    }

    #[test]
    fn children_follow_their_parent_with_depth() {
        let processes = [process(1, 0), process(2, 1), process(3, 2), process(4, 1)];
        let rows = tree(&processes, &HashSet::new());
        assert_eq!(pids(&rows), [1, 2, 3, 4]);
        assert_eq!(
            rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
            [0, 1, 2, 1]
        );
        assert_eq!(rows[0].children, 2);
    }

    #[test]
    fn collapsing_hides_the_subtree_but_keeps_its_root() {
        let processes = [process(1, 0), process(2, 1), process(3, 2)];
        let rows = tree(&processes, &HashSet::from([2]));
        assert_eq!(pids(&rows), [1, 2]);
        assert!(rows[1].collapsed);
    }

    #[test]
    fn sibling_order_follows_the_input_order() {
        let processes = [process(9, 1), process(1, 0), process(5, 1)];
        assert_eq!(pids(&tree(&processes, &HashSet::new())), [1, 9, 5]);
    }

    #[test]
    fn orphans_start_their_own_trees() {
        let processes = [process(2, 99), process(3, 2)];
        assert_eq!(pids(&tree(&processes, &HashSet::new())), [2, 3]);
    }

    #[test]
    fn looping_parent_links_still_show_each_process_once() {
        let processes = [process(2, 3), process(3, 2), process(4, 4)];
        assert_eq!(pids(&tree(&processes, &HashSet::new())), [4, 2, 3]);
    }

    #[test]
    fn the_filter_keeps_matching_rows_flat() {
        let processes = [process(1, 0), process(2, 1)];
        assert_eq!(pids(&flat(&processes, |p| p.pid == 2)), [2]);
    }
}
