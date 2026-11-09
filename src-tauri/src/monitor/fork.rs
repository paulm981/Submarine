//! The fork's own parts of monitor.rs, moved here (#215) so a sync with the
//! original project doesn't conflict on them: what known_hosts says about a
//! node (`KnownHosts`), a connection kept with its jump host's
//! (`MonitorSession`), the node as `~/.ssh/config` and known_hosts resolve it
//! with its jump host (`config_target`, #43), and the handler methods that
//! refuse every channel a server tries to open (`refuse_server_channels!`).
//! monitor.rs brings it in with `use fork::*`.

use super::*;

/// A host's known_hosts entries, carried with its `NodeAuth`.
#[derive(Clone, Default)]
pub struct KnownHosts(pub(crate) Option<Arc<crate::known_hosts::HostTrust>>);

impl std::fmt::Debug for KnownHosts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "KnownHosts(read)" } else { "KnownHosts(none)" })
    }
}

/// A monitor's SSH connection, with the jump host's when it goes through one:
/// that connection carries this one, so it has to stay open as long.
pub(crate) struct MonitorSession {
    pub(crate) session: russh::client::Handle<MonitorHandler>,
    pub(crate) jump: Option<russh::client::Handle<MonitorHandler>>,
}

impl MonitorSession {
    pub(crate) fn is_closed(&self) -> bool {
        self.session.is_closed() || self.jump.as_ref().is_some_and(|j| j.is_closed())
    }
}

/// The node as `~/.ssh/config` resolves it: an alias's HostName and Port, and
/// its User when none is saved; its jump host's too, as the terminal resolves
/// it (`ConfigTarget::resolve_jump_host`), so the monitor dials the address
/// the terminal does and finds the host key the terminal stored under it.
/// The monitor takes nothing else from the config (no ProxyCommand /
/// ProxyJump, key files or agent): in the background it never runs commands
/// or asks for anything. A node the config sends through a ProxyCommand or
/// ProxyJump (and that has no proxy of its own), or a jump host it sends
/// through either, isn't checked at all: `Err` says why. The terminal goes
/// through them (#169), so dialling past them would check another path.
///
/// A server (or jump host) that proves who it is only with a certificate
/// isn't checked either: the monitor trusts host keys the app has on file,
/// and a certificate is never put there (trust_policy.rs, #43 D4).
pub(crate) async fn config_target(
    db: &Arc<std::sync::Mutex<Option<rusqlite::Connection>>>,
    auth: &NodeAuth,
    node_name: &str,
) -> Result<NodeAuth, String> {
    let mut target = crate::ConfigTarget::resolve(&auth.host, auth.port, &auth.username, Some(node_name.to_string())).await;
    let node_trust = known_hosts_for(&mut target).await.map(Arc::new);
    let mut jump = match auth.jump.as_deref() {
        Some(j) => Some(
            crate::ConfigTarget::resolve_jump_host(&j.host, i32::from(j.port), &j.username, j.name.clone())
                .await
                .map_err(|e| format!("jump host {}: {}", j.host, e))?,
        ),
        None => None,
    };
    let jump_trust = match jump.as_mut() {
        Some((j, _)) => known_hosts_for(j).await.map(Arc::new),
        None => None,
    };
    let mut resolved = apply_ssh_config(auth, target, jump)?;
    resolved.known_hosts = KnownHosts(node_trust.clone());
    if let Some(j) = resolved.jump.as_deref_mut() {
        j.known_hosts = KnownHosts(jump_trust.clone());
    }
    if let Ok(guard) = db.lock() {
        if let Some(conn) = guard.as_ref() {
            let only_cert = |trust: &Option<Arc<crate::known_hosts::HostTrust>>, host: &str, port: u16| {
                crate::trust_policy::known_only_by_certificate(trust.as_deref(), conn, host, port)
            };
            if only_cert(&node_trust, &resolved.host, resolved.port) {
                return Err(NOT_CHECKED_CERTIFICATE.into());
            }
            if let Some(j) = resolved.jump.as_deref() {
                if only_cert(&jump_trust, &j.host, j.port) {
                    return Err(NOT_CHECKED_JUMP_CERTIFICATE.into());
                }
            }
        }
    }
    Ok(resolved)
}

/// What the user's known_hosts files say about `t` (None with the
/// ~/.ssh/config switch off).
pub(crate) async fn known_hosts_for(t: &mut crate::ConfigTarget) -> Option<crate::known_hosts::HostTrust> {
    let user = t.user.clone();
    let files = t.known_hosts_files(&user);
    crate::known_hosts::for_host(files, &t.host, t.port).await
}

/// `config_target` once the config is read: `t` is the node's, `jump` its
/// jump host's, with the ProxyCommand or ProxyJump the config gives it.
pub(crate) fn apply_ssh_config(
    auth: &NodeAuth,
    mut t: crate::ConfigTarget,
    jump: Option<(crate::ConfigTarget, Option<crate::ssh_config::Proxy>)>,
) -> Result<NodeAuth, String> {
    let own_route = match auth.proxy_type.as_str() {
        "socks5" => Some("SOCKS5 proxy"),
        "http" => Some("HTTP proxy"),
        // Its saved jump host (connect_for_monitor goes through it).
        _ if auth.jump.is_some() => Some("jump host"),
        _ => None,
    };
    match t.proxy(own_route) {
        Some(crate::ssh_config::Proxy::Command(_)) => return Err(NOT_CHECKED_PROXY_COMMAND.into()),
        Some(crate::ssh_config::Proxy::Jump(_)) => return Err(NOT_CHECKED_PROXY_JUMP.into()),
        _ => {}
    }
    let jump = match (auth.jump.as_deref(), jump) {
        (Some(saved), Some((j, proxy))) => {
            // The terminal reaches the jump host through the ProxyCommand or
            // ProxyJump the config gives it; the monitor runs neither (#43, D5).
            match proxy {
                Some(crate::ssh_config::Proxy::Command(_)) => return Err(NOT_CHECKED_JUMP_PROXY_COMMAND.into()),
                Some(crate::ssh_config::Proxy::Jump(_)) => return Err(NOT_CHECKED_JUMP_PROXY_JUMP.into()),
                _ => {}
            }
            Some(Box::new(NodeAuth { host: j.host, port: j.port, username: j.user, ..saved.clone() }))
        }
        _ => None,
    };
    Ok(NodeAuth { host: t.host, port: t.port, username: t.user, jump, ..auth.clone() })
}

pub(crate) const NOT_CHECKED_JUMP_PROXY_COMMAND: &str =
    "Not checked: ~/.ssh/config reaches this server's jump host through a ProxyCommand, which status checks don't run.";
pub(crate) const NOT_CHECKED_CERTIFICATE: &str =
    "Not checked: this server proves who it is with a certificate (an @cert-authority line in your known_hosts), and status checks only trust host keys saved in the app.";
pub(crate) const NOT_CHECKED_JUMP_CERTIFICATE: &str =
    "Not checked: this server's jump host proves who it is with a certificate (an @cert-authority line in your known_hosts), and status checks only trust host keys saved in the app.";
pub(crate) const NOT_CHECKED_JUMP_PROXY_JUMP: &str =
    "Not checked: ~/.ssh/config reaches this server's jump host through a ProxyJump, which status checks don't use.";

/// MonitorHandler's methods that refuse every channel the server tries to
/// open, for use inside its `impl client::Handler`: a monitor connection only
/// opens its own exec channels, as ClientHandler does (ssh_manager.rs), and
/// russh's defaults accept them.
macro_rules! refuse_server_channels {
    () => {
            // A monitor connection only opens its own exec channels, so every
            // channel the server tries to open is refused, as ClientHandler does
            // (ssh_manager.rs). russh's defaults accept them.
            async fn server_channel_open_session(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }

            async fn server_channel_open_direct_tcpip(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                _host_to_connect: &str,
                _port_to_connect: u32,
                _originator_address: &str,
                _originator_port: u32,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }

            async fn server_channel_open_forwarded_tcpip(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                _connected_address: &str,
                _connected_port: u32,
                _originator_address: &str,
                _originator_port: u32,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }

            async fn server_channel_open_direct_streamlocal(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                _socket_path: &str,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }

            async fn server_channel_open_forwarded_streamlocal(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                _socket_path: &str,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }

            async fn server_channel_open_agent_forward(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }

            async fn server_channel_open_x11(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                _originator_address: &str,
                _originator_port: u32,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }

            async fn server_channel_open_unknown(
                &mut self,
                _channel: russh::Channel<client::Msg>,
                reply: client::ChannelOpenHandle,
                _session: &mut client::Session,
            ) -> Result<(), Self::Error> {
                reply.reject(russh::ChannelOpenFailure::AdministrativelyProhibited).await;
                Ok(())
            }
    };
}
pub(crate) use refuse_server_channels;

// The lines that bring this in, and use it, in monitor.rs (fork_hooks.rs).
#[cfg(test)]
mod hook_tests {
    #[test]
    fn the_hooks_are_in_place() {
        crate::fork_hooks::assert_hooks(
            "monitor.rs",
            include_str!("../monitor.rs"),
            &[("", "mod fork;", 1), ("", "pub(crate) use fork::*;", 1), ("", "refuse_server_channels!();", 1), ("run_poller", "config_target(", 1)],
        );
    }
}
