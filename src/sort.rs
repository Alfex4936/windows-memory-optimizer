use crate::memory::Process;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Column {
    Name,
    Pid,
    Memory,
    Cpu,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessSort {
    pub column: Column,
    pub descending: bool,
}

impl Default for ProcessSort {
    fn default() -> Self {
        Self {
            column: Column::Memory,
            descending: true,
        }
    }
}

impl ProcessSort {
    pub fn select(&mut self, column: Column) {
        self.descending = if self.column == column {
            !self.descending
        } else {
            matches!(column, Column::Memory | Column::Cpu)
        };
        self.column = column;
    }

    pub fn sort(&self, processes: &mut [&Process]) {
        processes.sort_by(|a, b| {
            if self.column == Column::Cpu && a.cpu_percent.is_some() != b.cpu_percent.is_some() {
                return b.cpu_percent.is_some().cmp(&a.cpu_percent.is_some());
            }
            let order = match self.column {
                Column::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Column::Pid => a.pid.cmp(&b.pid),
                Column::Memory => a.bytes.cmp(&b.bytes),
                Column::Cpu => a
                    .cpu_percent
                    .unwrap_or(0.0)
                    .total_cmp(&b.cpu_percent.unwrap_or(0.0)),
                Column::Status => (!a.eligible, &a.reason).cmp(&(!b.eligible, &b.reason)),
            };
            let order = if self.descending {
                order.reverse()
            } else {
                order
            };
            order
                .then_with(|| a.pid.cmp(&b.pid))
                .then_with(|| a.created.cmp(&b.created))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, name: &str, bytes: u64, eligible: bool, reason: &str) -> Process {
        Process {
            pid,
            name: name.into(),
            path: format!(r"D:\Apps\{name}"),
            bytes,
            cpu_percent: None,
            created: u64::from(pid),
            eligible,
            reason: reason.into(),
        }
    }

    #[test]
    fn cpu_sort_uses_numeric_values_keeps_unknown_last_and_stable_identity_ties() {
        let mut source = [
            process(10, "app.exe", 1, true, "ready"),
            process(1, "app.exe", 1, true, "ready"),
            process(2, "app.exe", 1, true, "ready"),
            process(3, "app.exe", 1, true, "ready"),
        ];
        source[0].cpu_percent = Some(2.5);
        source[1].cpu_percent = Some(10.0);
        source[2].cpu_percent = Some(2.5);
        for (descending, expected) in [(false, [2, 10, 1, 3]), (true, [1, 2, 10, 3])] {
            let mut rows: Vec<_> = source.iter().collect();
            ProcessSort {
                column: Column::Cpu,
                descending,
            }
            .sort(&mut rows);
            assert_eq!(rows.iter().map(|p| p.pid).collect::<Vec<_>>(), expected);
        }
        let mut sort = ProcessSort::default();
        sort.select(Column::Cpu);
        assert!(sort.descending);
        sort.select(Column::Cpu);
        assert!(!sort.descending);
    }

    #[test]
    fn every_column_sorts_in_both_directions_with_numeric_values_and_stable_name_ties() {
        let source = [
            process(2, "app.exe", 10, true, "선택하여 정리"),
            process(11, "Beta.exe", 2, false, "Windows 구성 요소"),
            process(1, "APP.exe", 1000, true, "선택하여 정리"),
            process(3, "gamma.exe", 100, false, "현재 사용 중인 앱"),
        ];
        for (column, ascending, descending) in [
            (Column::Name, [1, 2, 11, 3], [3, 11, 1, 2]),
            (Column::Pid, [1, 2, 3, 11], [11, 3, 2, 1]),
            (Column::Memory, [11, 2, 3, 1], [1, 3, 2, 11]),
            (Column::Status, [1, 2, 11, 3], [3, 11, 1, 2]),
        ] {
            for (descending, expected) in [(false, ascending), (true, descending)] {
                let mut rows: Vec<_> = source.iter().collect();
                ProcessSort { column, descending }.sort(&mut rows);
                assert_eq!(rows.iter().map(|p| p.pid).collect::<Vec<_>>(), expected);
            }
        }
    }

    #[test]
    fn refresh_order_does_not_reorder_equal_values_or_reverse_identity_ties() {
        let mut older = process(4, "same.exe", 50, true, "ready");
        older.created = 10;
        let mut newer = older.clone();
        newer.created = 20;
        let other = process(2, "same.exe", 50, true, "ready");
        for descending in [false, true] {
            let sort = ProcessSort {
                column: Column::Memory,
                descending,
            };
            let mut first = vec![&newer, &other, &older];
            let mut refreshed = vec![&older, &newer, &other];
            sort.sort(&mut first);
            sort.sort(&mut refreshed);
            let identities =
                |rows: &[&Process]| rows.iter().map(|p| (p.pid, p.created)).collect::<Vec<_>>();
            assert_eq!(identities(&first), vec![(2, 2), (4, 10), (4, 20)]);
            assert_eq!(identities(&refreshed), identities(&first));
        }
    }

    #[test]
    fn header_selection_toggles_direction_and_sets_column_defaults() {
        let mut sort = ProcessSort::default();
        assert_eq!(sort.column, Column::Memory);
        assert!(sort.descending);
        sort.select(Column::Memory);
        assert!(!sort.descending);
        sort.select(Column::Memory);
        assert!(sort.descending);
        for column in [Column::Name, Column::Pid, Column::Status] {
            sort.select(column);
            assert_eq!(sort.column, column);
            assert!(!sort.descending);
            sort.select(column);
            assert!(sort.descending);
        }
        sort.select(Column::Memory);
        assert!(sort.descending);
    }
}
