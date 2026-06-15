// SPDX-FileCopyrightText: 2023 MTRNord
//
// SPDX-License-Identifier: Apache-2.0

/// Admin-facing queries for eroosterctl
pub mod admin;

/// The database logic of the server
pub mod database;

/// Persistent outbound mail queue
pub mod queue;

/// The logic for the mail storages
pub mod storage;
