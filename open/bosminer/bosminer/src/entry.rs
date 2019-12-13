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

//! This module provides top level functionality to parse command line options and run the
//! mining protocol client (= bosminer frontend) and connect it with provided hardware
//! specific backend.

use crate::api;
use crate::client;
use crate::hal;
use crate::hub;
use crate::runtime_config;
use crate::stats;

use ii_async_compat::tokio;

use std::sync::Arc;

pub async fn main<T: hal::Backend>() {
    let _log_guard = ii_logging::setup_for_app();

    // Set default backend midstate count
    runtime_config::set_midstate_count(T::DEFAULT_MIDSTATE_COUNT);

    // Initialize hub core which manages all resources
    let core = Arc::new(hub::Core::new());

    // Create and initialize the backend
    let mut configuration = core
        .add_backend::<T>()
        .await
        .expect("Backend initialization failed");

    tokio::spawn(core.clone().run());
    // start statistics processing
    tokio::spawn(stats::mining_task(
        core.frontend.clone(),
        T::DEFAULT_HASHRATE_INTERVAL,
    ));

    // start client based on user input
    for client_descriptor in configuration.clients.drain(..) {
        client::register(&core, client_descriptor).await.enable();
    }

    // the bosminer is controlled with API which also controls when the miner will end
    api::run(core, configuration).await;
}
