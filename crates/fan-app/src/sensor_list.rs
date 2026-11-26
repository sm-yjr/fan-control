//! Pure sensor grouping, search filtering, and hottest-first sorting shared by
//! the GPUI sensor details page and unit tests.
#[cfg(test)]
use fan_core::Sensor;
use fan_core::SensorGroup;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum GroupFilter {
    #[default]
    All,
    Cpu,
    Gpu,
    System,
    Other,
}

impl GroupFilter {
    pub const ALL: [Self; 5] = [Self::All, Self::Cpu, Self::Gpu, Self::System, Self::Other];

    #[allow(dead_code)]
    pub fn title(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Cpu => "处理器",
            Self::Gpu => "图形",
            Self::System => "机身",
            Self::Other => "其他",
        }
    }

    #[allow(dead_code)]
    pub fn matches(self, group: SensorGroup) -> bool {
        match self {
            Self::All => true,
            Self::Cpu => group == SensorGroup::Cpu,
            Self::Gpu => group == SensorGroup::Gpu,
            Self::System => group == SensorGroup::System,
            Self::Other => group == SensorGroup::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SortOrder {
    #[default]
    Grouped,
    HottestFirst,
}

impl SortOrder {
    pub const ALL: [Self; 2] = [Self::Grouped, Self::HottestFirst];

    #[allow(dead_code)]
    pub fn title(self) -> &'static str {
        match self {
            Self::Grouped => "按分组",
            Self::HottestFirst => "按最热",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Entry {
        Header(String),
        Reading(Sensor),
    }

    impl Entry {
        fn same_layout(&self, other: &Self) -> bool {
            match (self, other) {
                (Self::Header(a), Self::Header(b)) => a == b,
                (Self::Reading(a), Self::Reading(b)) => {
                    a.key == b.key && a.name == b.name && a.group == b.group
                }
                _ => false,
            }
        }
    }

    fn group_rank(group: SensorGroup) -> (u8, &'static str) {
        match group {
            SensorGroup::Cpu => (0, "处理器"),
            SensorGroup::Gpu => (1, "图形处理器"),
            SensorGroup::System => (2, "机身内部（不代表键盘表面温度）"),
            SensorGroup::Other => (3, "其他"),
        }
    }

    fn is_headroom(sensor: &Sensor) -> bool {
        sensor.name.contains("热余量")
    }

    fn entries(
        sensors: &[Sensor],
        query: &str,
        filter: GroupFilter,
        sort: SortOrder,
        fresh: bool,
    ) -> Vec<Entry> {
        let needle = query.trim().to_lowercase();
        let mut filtered: Vec<Sensor> = sensors
            .iter()
            .filter(|s| filter.matches(s.group))
            .filter(|s| {
                needle.is_empty()
                    || s.name.to_lowercase().contains(&needle)
                    || s.key.to_lowercase().contains(&needle)
            })
            .map(|s| Sensor {
                key: s.key.clone(),
                name: s.name.clone(),
                group: s.group,
                value: if fresh {
                    s.value.filter(|v| v.is_finite())
                } else {
                    None
                },
            })
            .collect();

        if filtered.is_empty() {
            return vec![Entry::Header("没有匹配的传感器。".into())];
        }

        if sort == SortOrder::HottestFirst {
            filtered.sort_by(|a, b| {
                is_headroom(a)
                    .cmp(&is_headroom(b))
                    .then_with(|| b.value.unwrap_or(-1.0).total_cmp(&a.value.unwrap_or(-1.0)))
                    .then_with(|| a.name.cmp(&b.name))
            });
            return filtered.into_iter().map(Entry::Reading).collect();
        }

        filtered.sort_by(|a, b| {
            group_rank(a.group)
                .0
                .cmp(&group_rank(b.group).0)
                .then_with(|| a.name.cmp(&b.name))
        });

        let mut rows = Vec::new();
        let mut current: Option<u8> = None;
        for s in filtered {
            let (rank, title) = group_rank(s.group);
            if current != Some(rank) {
                rows.push(Entry::Header(title.into()));
                current = Some(rank);
            }
            rows.push(Entry::Reading(s));
        }
        rows
    }

    fn sensor(key: &str, group: SensorGroup, value: Option<f64>) -> Sensor {
        Sensor {
            key: key.into(),
            name: format!("Sensor {key}"),
            group,
            value,
        }
    }

    #[test]
    fn grouped_search_retains_identity_and_validity() {
        let sensors = [
            sensor("ZZ01", SensorGroup::Other, Some(40.)),
            sensor("Tp01", SensorGroup::Cpu, Some(65.)),
            sensor("Tp02", SensorGroup::Cpu, Some(f64::NAN)),
        ];
        let rows = entries(&sensors, "", GroupFilter::All, SortOrder::Grouped, true);
        assert!(matches!(&rows[1], Entry::Reading(s) if s.key == "Tp01" && s.value == Some(65.)));
        assert!(matches!(&rows[2], Entry::Reading(s) if s.key == "Tp02" && s.value.is_none()));
        assert_eq!(
            entries(&sensors, "tp01", GroupFilter::All, SortOrder::Grouped, true).len(),
            2
        );
        assert_eq!(
            entries(
                &sensors,
                "sensor zz",
                GroupFilter::All,
                SortOrder::Grouped,
                true
            )
            .len(),
            2
        );
        assert_eq!(
            entries(
                &sensors,
                "absent",
                GroupFilter::All,
                SortOrder::Grouped,
                true
            ),
            vec![Entry::Header("没有匹配的传感器。".into())]
        );
        assert!(
            entries(&sensors, "", GroupFilter::All, SortOrder::Grouped, false)
                .iter()
                .all(|row| !matches!(row, Entry::Reading(s) if s.value.is_some()))
        );
    }

    #[test]
    fn group_filter_and_hottest_sort_work_accurately() {
        let sensors = [
            sensor("ZZ01", SensorGroup::Other, Some(40.)),
            sensor("Tp01", SensorGroup::Cpu, Some(65.)),
            sensor("Tg01", SensorGroup::Gpu, Some(82.)),
            Sensor {
                key: "Th01".into(),
                name: "CPU 热余量（非绝对温度）".into(),
                group: SensorGroup::Cpu,
                value: Some(95.),
            },
        ];
        let cpu_only = entries(&sensors, "", GroupFilter::Cpu, SortOrder::Grouped, true);
        assert_eq!(cpu_only.len(), 3);

        let hottest = entries(
            &sensors,
            "",
            GroupFilter::All,
            SortOrder::HottestFirst,
            true,
        );
        assert_eq!(hottest.len(), 4);
        assert!(matches!(&hottest[0], Entry::Reading(s) if s.key == "Tg01"));
        assert!(matches!(&hottest[1], Entry::Reading(s) if s.key == "Tp01"));
        assert!(matches!(&hottest[2], Entry::Reading(s) if s.key == "ZZ01"));
        assert!(matches!(&hottest[3], Entry::Reading(s) if s.key == "Th01"));
    }

    #[test]
    fn readings_do_not_invalidate_layout_but_semantics_do() {
        let first = Entry::Reading(sensor("Tp01", SensorGroup::Cpu, Some(65.)));
        let mut next = sensor("Tp01", SensorGroup::Cpu, None);
        assert!(first.same_layout(&Entry::Reading(next.clone())));
        next.name = "Confirmed meaning".into();
        assert!(!first.same_layout(&Entry::Reading(next.clone())));
        next.name = "Sensor Tp01".into();
        next.group = SensorGroup::Other;
        assert!(!first.same_layout(&Entry::Reading(next)));
    }
}
