//! stores per-instance ICE sessions and indexes their UDP routes.

use crate::stun;
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

// caps session state when clients never complete or close connections.
const MAX_SESSIONS: usize = 1024;
// limits authenticated client addresses retained by each session.
const MAX_CLIENT_ADDRS: usize = 16;
// expires sessions after five minutes without traffic.
const IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// holds the credentials and routes required for one player connection.
struct Session {
    /// stores the player's offer ufrag.
    client_ufrag: String,
    /// stores the BDS answer ufrag.
    server_ufrag: String,
    /// authenticates STUN requests with MESSAGE-INTEGRITY.
    ice_password: String,
    /// lists the BDS UDP candidates in preference order.
    bds_targets: Vec<SocketAddr>,
    /// contains client addresses verified by STUN.
    client_addrs: HashSet<SocketAddr>,
    /// identifies the client destination for BDS replies.
    active_client: Option<SocketAddr>,
    /// tracks activity for expiration.
    last_seen: Instant,
}

/// groups route indexes so packet lookup avoids scanning every session.
#[derive(Default)]
struct SessionState {
    sessions: HashMap<String, Session>,
    client_routes: HashMap<SocketAddr, String>,
    bds_routes: HashMap<SocketAddr, String>,
    ufrag_routes: HashMap<String, String>,
}

/// owns the session table for a single BDS instance.
#[derive(Default)]
pub struct SessionTable {
    state: Mutex<SessionState>,
}

impl SessionTable {
    /// registers ICE credentials and BDS candidates after a successful exchange.
    pub fn register(
        &self,
        network_id: String,
        client_ufrag: String,
        server_ufrag: String,
        ice_password: String,
        bds_targets: Vec<SocketAddr>,
    ) -> Result<(), String> {
        if client_ufrag.is_empty() || server_ufrag.is_empty() || ice_password.is_empty() {
            return Err("SDP missing ICE ufrag or ice-pwd".to_owned());
        }
        if bds_targets.is_empty() {
            return Err("SDP answer has no available UDP candidate".to_owned());
        }

        let mut unique_targets = HashSet::new();
        let bds_targets: Vec<_> = bds_targets
            .into_iter()
            .filter(|target| unique_targets.insert(*target))
            .collect();
        let mut state = self.state.lock().map_err(|_| "Failed to lock the session table".to_owned())?;

        if !state.sessions.contains_key(&network_id) && state.sessions.len() >= MAX_SESSIONS {
            return Err("Maximum concurrency limit".to_owned());
        }
        for target in &bds_targets {
            if state
                .bds_routes
                .get(target)
                .is_some_and(|owner| owner != &network_id)
            {
                return Err(format!("BDS candidate Address occupied:{target}"));
            }
        }
        for ufrag in [&client_ufrag, &server_ufrag] {
            if state
                .ufrag_routes
                .get(ufrag)
                .is_some_and(|owner| owner != &network_id)
            {
                return Err("ICE ufrag occupied".to_owned());
            }
        }

        // replace old indexes only after all new routes pass conflict checks.
        remove_session_indexes(&mut state, &network_id);
        for target in &bds_targets {
            state.bds_routes.insert(*target, network_id.clone());
        }
        state
            .ufrag_routes
            .insert(client_ufrag.clone(), network_id.clone());
        state
            .ufrag_routes
            .insert(server_ufrag.clone(), network_id.clone());
        state.sessions.insert(
            network_id,
            Session {
                client_ufrag,
                server_ufrag,
                ice_password,
                bds_targets,
                client_addrs: HashSet::new(),
                active_client: None,
                last_seen: Instant::now(),
            },
        );
        Ok(())
    }

    /// refreshes a known client route and returns its BDS destination.
    pub fn relay_from_client(&self, source: SocketAddr) -> Option<SocketAddr> {
        let mut state = self.state.lock().ok()?;
        let network_id = state.client_routes.get(&source)?.clone();
        let session = state.sessions.get_mut(&network_id)?;
        session.active_client = Some(source);
        session.last_seen = Instant::now();
        session.bds_targets.first().copied()
    }

    /// refreshes a known BDS route and returns its active client.
    pub fn relay_from_bds(&self, source: SocketAddr) -> Option<SocketAddr> {
        let mut state = self.state.lock().ok()?;
        let network_id = state.bds_routes.get(&source)?.clone();
        let session = state.sessions.get_mut(&network_id)?;
        session.last_seen = Instant::now();
        session.active_client
    }

    /// verifies an ICE binding request before claiming its source address.
    pub fn claim_client(
        &self,
        source: SocketAddr,
        packet: &[u8],
    ) -> Result<Option<SocketAddr>, &'static str> {
        let Some(username) = stun::username(packet) else {
            return Ok(None);
        };
        let Some((first_ufrag, second_ufrag)) = username.split_once(':') else {
            return Ok(None);
        };

        let mut state = self.state.lock().map_err(|_| "Session table locking failed!")?;
        let Some(network_id) = state.ufrag_routes.get(first_ufrag).cloned() else {
            return Ok(None);
        };
        if state.ufrag_routes.get(second_ufrag) != Some(&network_id) {
            return Ok(None);
        }
        let Some(session) = state.sessions.get(&network_id) else {
            return Ok(None);
        };
        let expected_pair = (first_ufrag == session.server_ufrag
            && second_ufrag == session.client_ufrag)
            || (first_ufrag == session.client_ufrag && second_ufrag == session.server_ufrag);
        if !expected_pair {
            return Ok(None);
        }
        if !stun::verify_message_integrity(packet, &session.ice_password) {
            return Err("STUN MESSAGE-INTEGRITY Verification failed!");
        }
        if session.client_addrs.len() >= MAX_CLIENT_ADDRS && !session.client_addrs.contains(&source)
        {
            return Err("The maximum number of client addresses for a single session has been reached!");
        }

        if let Some(previous_owner) = state.client_routes.get(&source).cloned()
            && previous_owner != network_id
            && let Some(previous_session) = state.sessions.get_mut(&previous_owner)
        {
            previous_session.client_addrs.remove(&source);
            if previous_session.active_client == Some(source) {
                previous_session.active_client = None;
            }
        }

        let session = state
            .sessions
            .get_mut(&network_id)
            .ok_or("Inconsistent session table status!")?;
        session.client_addrs.insert(source);
        session.active_client = Some(source);
        session.last_seen = Instant::now();
        let target = session.bds_targets.first().copied();
        state.client_routes.insert(source, network_id);
        Ok(target)
    }

    /// removes expired sessions and all indexes that point to them.
    pub fn remove_idle_sessions(&self) {
        if let Ok(mut state) = self.state.lock() {
            let expired: Vec<_> = state
                .sessions
                .iter()
                .filter(|(_, session)| session.last_seen.elapsed() >= IDLE_TIMEOUT)
                .map(|(network_id, _)| network_id.clone())
                .collect();
            for network_id in expired {
                remove_session_indexes(&mut state, &network_id);
            }
        }
    }
}

/// removes every route index owned by the specified session.
fn remove_session_indexes(state: &mut SessionState, network_id: &str) {
    if let Some(session) = state.sessions.remove(network_id) {
        for address in session.client_addrs {
            state.client_routes.remove(&address);
        }
        for target in session.bds_targets {
            state.bds_routes.remove(&target);
        }
        state.ufrag_routes.remove(&session.client_ufrag);
        state.ufrag_routes.remove(&session.server_ufrag);
    }
}
