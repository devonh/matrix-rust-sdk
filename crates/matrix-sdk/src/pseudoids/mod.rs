// Copyright 2023 The Matrix.org Foundation C.I.C.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! High-level PseudoID API.
//!
//! Provides an abstraction over the pseudoIDs managed by the olm machine.

#![cfg_attr(target_arch = "wasm32", allow(unused_imports))]

use matrix_sdk_base::crypto::vodozemac::Ed25519SecretKey;

use crate::{Client, Error};

impl Client {}

#[cfg(any(feature = "testing", test))]
impl Client {}

/// A high-level API to manage the client's pseudoids.
///
/// To get this, use [`Client::pseudoids()`].
#[derive(Debug, Clone)]
pub struct PseudoIDs {
    /// The underlying client.
    client: Client,
}

impl PseudoIDs {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// Creates a new pseudoid for the room.
    pub async fn create_pseudoid_for_room(&self, room: &str) -> Result<Ed25519SecretKey, Error> {
        let olm = self.client.olm_machine().await;
        let machine = olm.as_ref().ok_or(Error::NoOlmMachine)?;
        Ok(machine.create_pseudoid_for_room(room).await)
    }

    /// Gets the existing pseudoid for a room if one exists.
    pub async fn get_pseudoid_for_room(&self, room: &str) -> Option<Ed25519SecretKey> {
        let olm = self.client.olm_machine().await;
        let machine = olm.as_ref()?;
        machine.get_pseudoid_for_room(room).await
    }
}
