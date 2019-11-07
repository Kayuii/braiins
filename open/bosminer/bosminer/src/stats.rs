// Copyright (C) 2019  Braiins Systems s.r.o.
//
// This file is part of Braiins Open-Source Initiative (BOSI).
//
// BOSI is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// Please, keep in mind that we may also license BOSI or any part thereof
// under a proprietary license. For more information on the terms and conditions
// of such proprietary license or if you have any other questions, please
// contact us at opensource@braiins.com.

use ii_logging::macros::*;

use crate::node;
use crate::stats;
use crate::work;

use bosminer_macros::MiningStats;

use ii_stats::WindowedTimeMean;

use futures::lock::Mutex;
use ii_async_compat::{futures, tokio};
use tokio::timer::delay_for;

use std::time;

use lazy_static::lazy_static;

lazy_static! {
    static ref DEFAULT_TIME_MEAN_INTERVALS: Vec<time::Duration> = vec![
        time::Duration::from_secs(5),
        time::Duration::from_secs(1 * 60),
        time::Duration::from_secs(5 * 60),
        time::Duration::from_secs(15 * 60),
        time::Duration::from_secs(24 * 60 * 60),
    ];
}

#[derive(Debug, Clone)]
pub struct MeterSnapshot {
    /// Number of solutions measured from the beginning of the mining
    pub solutions: u64,
    /// All shares measured from the beginning of the mining
    pub shares: ii_bitcoin::Shares,
    /// Approximate arithmetic mean of hashes within given time intervals (in kH/time)
    time_means: Vec<WindowedTimeMean>,
}

impl MeterSnapshot {
    fn get_time_mean(&self, interval: time::Duration) -> &WindowedTimeMean {
        self.time_means
            .iter()
            .find(|time_mean| time_mean.interval() == interval)
            .expect("cannot find given time interval")
    }

    #[inline]
    pub fn to_kilo_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        ii_bitcoin::HashesUnit::KiloHashes(self.get_time_mean(interval).measure(now))
    }

    #[inline]
    pub fn to_mega_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_mega_hashes()
    }

    #[inline]
    pub fn to_giga_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_giga_hashes()
    }

    #[inline]
    pub fn to_tera_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_tera_hashes()
    }

    #[inline]
    pub fn to_pretty_hashes(
        &self,
        interval: time::Duration,
        now: time::Instant,
    ) -> ii_bitcoin::HashesUnit {
        self.to_kilo_hashes(interval, now).into_pretty_hashes()
    }
}

#[derive(Debug)]
pub struct Meter {
    inner: Mutex<MeterSnapshot>,
}

impl Meter {
    pub fn new(intervals: &Vec<time::Duration>) -> Self {
        Self {
            inner: Mutex::new(MeterSnapshot {
                solutions: 0,
                shares: Default::default(),
                time_means: intervals
                    .iter()
                    .map(|&interval| WindowedTimeMean::new(interval))
                    .collect(),
            }),
        }
    }

    pub async fn take_snapshot(&self) -> MeterSnapshot {
        self.inner.lock().await.clone()
    }

    pub(crate) async fn account_solution(&self, target: &ii_bitcoin::Target, time: time::Instant) {
        let mut meter = self.inner.lock().await;
        let kilo_hashes = ii_bitcoin::Shares::new(target)
            .into_kilo_hashes()
            .into_f64();

        // TODO: what to do when number overflows
        meter.solutions += 1;
        meter.shares.account_solution(target);
        for time_mean in &mut meter.time_means {
            time_mean.insert(kilo_hashes, time);
        }
    }
}

impl Default for Meter {
    fn default() -> Self {
        Self::new(DEFAULT_TIME_MEAN_INTERVALS.as_ref())
    }
}

pub trait Mining: Send + Sync {
    /// The time all statistics are measured from
    fn start_time(&self) -> &time::Instant;
    /// Statistics for all valid blocks on network difficulty
    fn valid_network_diff(&self) -> &Meter;
    /// Statistics for all valid jobs on job/pool difficulty
    fn valid_job_diff(&self) -> &Meter;
    /// Statistics for all valid work on backend difficulty
    fn valid_backend_diff(&self) -> &Meter;
    /// Statistics for all invalid work on backend difficulty (backend/HW error)
    fn error_backend_diff(&self) -> &Meter;
}

#[derive(Debug, MiningStats)]
pub struct BasicMining {
    #[member_start_time]
    pub start_time: time::Instant,
    #[member_valid_network_diff]
    pub valid_network_diff: Meter,
    #[member_valid_job_diff]
    pub valid_job_diff: Meter,
    #[member_valid_backend_diff]
    pub valid_backend_diff: Meter,
    #[member_error_backend_diff]
    pub error_backend_diff: Meter,
}

impl BasicMining {
    pub fn new(start_time: time::Instant, intervals: &Vec<time::Duration>) -> Self {
        Self {
            start_time,
            valid_network_diff: Meter::new(&intervals),
            valid_job_diff: Meter::new(&intervals),
            valid_backend_diff: Meter::new(&intervals),
            error_backend_diff: Meter::new(&intervals),
        }
    }
}

impl Default for BasicMining {
    fn default() -> Self {
        Self::new(time::Instant::now(), DEFAULT_TIME_MEAN_INTERVALS.as_ref())
    }
}

/// Generate share accounting function for a particular difficulty level
/// The function traverses all nodes in the path and accounts the solution in the field specific
/// to the difficulty level given by `solution_target`
macro_rules! account_impl (
    ($name:ident, $field:ident) => (
        pub(crate) async fn $name(
            path: &node::Path,
            solution_target: &ii_bitcoin::Target,
            time: time::Instant,
        ) {
            for node in path {
                node.mining_stats()
                    .$field()
                    .account_solution(solution_target, time)
                    .await;
            }
        }
    )
);

account_impl!(account_valid_network_diff, valid_network_diff);
account_impl!(account_valid_job_diff, valid_job_diff);
account_impl!(account_valid_backend_diff, valid_backend_diff);
account_impl!(account_error_backend_diff, error_backend_diff);

/// Describes which difficulty target a particular solution has met.
/// It also determines in which statistics a particular solution should be accounted.
#[derive(Debug, PartialEq)]
pub enum DiffTargetType {
    Network,
    Job,
    Backend,
}

/// Accounts a valid `solution` to all relevant share accounting statistics based on
/// `met_diff_target_type`. Higher level DiffTargetType also belongs to all lower level types e.g.:
/// - solution that meets DiffTargetType::Network also belongs to DiffTargetType::{Job, Backend}
/// - solution that meets DiffTargetType::Job also belongs to DiffTargetType::Job accounts
pub async fn account_valid_solution(
    path: &node::Path,
    solution: &work::Solution,
    time: time::Instant,
    met_diff_target_type: DiffTargetType,
) {
    account_valid_backend_diff(path, solution.backend_target(), time).await;
    if met_diff_target_type != DiffTargetType::Backend {
        account_valid_job_diff(path, &solution.job_target(), time).await;
        if met_diff_target_type != DiffTargetType::Job {
            account_valid_network_diff(path, &solution.network_target(), time).await;
            assert_eq!(
                met_diff_target_type,
                DiffTargetType::Network,
                "BUG: unexpected difficulty target type"
            );
        }
    }
}

pub async fn mining_task(node: node::DynInfo, interval: time::Duration) {
    loop {
        delay_for(time::Duration::from_secs(1)).await;
        let valid_job_diff = node.mining_stats().valid_job_diff().take_snapshot().await;

        info!(
            "Hash rate @ pool difficulty: {}/{}s",
            valid_job_diff.to_pretty_hashes(interval, time::Instant::now()),
            interval.as_secs()
        );
    }
}
