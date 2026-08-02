//! Blocking TCP, through the handle table (ADR-016, ADR-045).
//!
//! This is the adapter that pays off ADR-042's deferred vocabulary. `:timeout`,
//! `:would-block`, and `:connection-reset` were named there and left out on the
//! grounds that a kind nobody can raise is a guess with a colon in front of it.
//! A file can produce none of the three; a socket produces all of them, so this
//! module is where they stop being hypothetical.
//!
//! Reading and writing are not here. A `Host::Tcp` is read by `io/read` and
//! written by `io/write` exactly as a file is, which is what the handle table
//! was for — an adapter that needed its own read primitive would be an adapter
//! that had not gone through the boundary.

use crate::host::{
    handle_arg, host_failed, io_fault, misuse, not_live, string_arg, Host, IoKind, IoOp,
};
use crate::value::{StrObj, Value};
use crate::vm::{Fault, Vm};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::time::Duration;

pub fn install(vm: &mut Vm) {
    vm.native("tcp/connect", 1, true, |vm, a| {
        let addr = string_arg(&a[0], "tcp/connect")?.to_string();
        // An optional timeout in milliseconds, because a connect with no
        // deadline is how a program hangs with no diagnostic — and `:timeout`
        // exists precisely so that case is dispatchable rather than fatal.
        let stream = match a.get(1) {
            None => TcpStream::connect(&addr).map_err(|e| ms_err(IoOp::Connect, &addr, e)),
            Some(Value::Int(ms)) if *ms >= 0 => {
                let one = resolve(&addr)?;
                TcpStream::connect_timeout(&one, Duration::from_millis(*ms as u64))
                    .map_err(|e| ms_err(IoOp::Connect, &addr, e))
            }
            Some(_) => Err(misuse(
                "`tcp/connect` takes an address and an optional timeout in milliseconds",
            )),
        }?;
        Ok(Value::Handle(vm.open_handle(Host::Tcp(stream))))
    });

    vm.native("tcp/listen", 1, false, |vm, a| {
        let addr = string_arg(&a[0], "tcp/listen")?.to_string();
        let l = TcpListener::bind(&addr).map_err(|e| ms_err(IoOp::Listen, &addr, e))?;
        Ok(Value::Handle(vm.open_handle(Host::Listener(l, None))))
    });

    // Blocking, like everything else here (ADR-042 part 3: a host call
    // completes or it throws, and the VM never suspends inside one) — a
    // deadline changes when it throws, not that it does.
    vm.native("tcp/accept", 1, false, |vm, a| {
        let id = handle_arg(&a[0], "tcp/accept")?;
        let accepted = match vm.host_mut(id).ok_or_else(|| not_live(IoOp::Accept))? {
            Host::Listener(l, None) => l.accept(),
            Host::Listener(l, Some(d)) => accept_before(l, *d)?,
            _ => return Err(misuse("`tcp/accept` needs a listener, not a connection")),
        };
        let (stream, _) = accepted.map_err(|e| host_failed(IoOp::Accept, None, &e))?;
        // ADR-059. Whether an accepted socket inherits the listener's
        // non-blocking mode is platform-specific, and the platform that says
        // yes would hand the program a connection whose every read fails with
        // `:would-block` for no reason visible from the language. Set it back
        // explicitly rather than relying on which platform this is.
        stream
            .set_nonblocking(false)
            .map_err(|e| host_failed(IoOp::Accept, None, &e))?;
        Ok(Value::Handle(vm.open_handle(Host::Tcp(stream))))
    });

    // The local address a listener actually bound to. Needed because binding
    // port 0 is the only way to ask the operating system for a free port, and a
    // test that hard-coded one would fail on a machine already using it.
    vm.native("tcp/local-addr", 1, false, |vm, a| {
        let id = handle_arg(&a[0], "tcp/local-addr")?;
        let addr = match vm.host_mut(id).ok_or_else(|| not_live(IoOp::Listen))? {
            Host::Listener(l, _) => l.local_addr(),
            Host::Tcp(t) => t.local_addr(),
            _ => return Err(misuse("`tcp/local-addr` needs a socket")),
        };
        let addr = addr.map_err(|e| host_failed(IoOp::Listen, None, &e))?;
        Ok(Value::Str(Rc::new(StrObj(addr.to_string()))))
    });

    // Read and write deadlines, so `:timeout` has a raiser a test can force.
    // `nil` clears one, which is the only way back to blocking forever.
    //
    // ADR-059 extends this to a listener, where it is the accept deadline.
    // One spelling for one concept: a program that has learned `set-timeout`
    // on a connection does not learn a second name for the socket it came
    // from.
    vm.native("tcp/set-timeout", 2, false, |vm, a| {
        let id = handle_arg(&a[0], "tcp/set-timeout")?;
        let d = match &a[1] {
            Value::Nil => None,
            Value::Int(ms) if *ms >= 0 => Some(Duration::from_millis(*ms as u64)),
            _ => {
                return Err(misuse(
                    "`tcp/set-timeout` takes milliseconds or nil to clear",
                ))
            }
        };
        match vm.host_mut(id).ok_or_else(|| not_live(IoOp::Read))? {
            Host::Tcp(t) => t
                .set_read_timeout(d)
                .and_then(|()| t.set_write_timeout(d))
                .map(|()| Value::Nil)
                .map_err(|e| host_failed(IoOp::Read, None, &e)),
            // Stored rather than pushed at the socket: there is no
            // `TcpListener::set_read_timeout` to push it to, which is the whole
            // reason ADR-059 needs a field and a loop instead of one call.
            Host::Listener(_, deadline) => {
                *deadline = d;
                Ok(Value::Nil)
            }
            _ => Err(misuse("`tcp/set-timeout` needs a socket")),
        }
    });
}

/// ADR-059: accept, or `:timeout`, within `d`.
///
/// Non-blocking plus a poll, because `SO_RCVTIMEO` — which does apply to
/// `accept` on Unix — is reachable only through `libc`, and ADR-014 makes a
/// dependency an ADR of its own. The listener is put back into blocking mode on
/// every path, including the timeout, so a deadline that expired leaves the
/// handle exactly as it found it.
///
/// The `:kind` is `:timeout` and not `:would-block` because this clock is ours.
/// `io/read`'s deadline forwards whatever the platform raised, and the platforms
/// disagree (TRAPS.md); nothing is being forwarded here.
fn accept_before(
    l: &TcpListener,
    d: Duration,
) -> Result<std::io::Result<(TcpStream, std::net::SocketAddr)>, Fault> {
    const POLL: Duration = Duration::from_millis(1);

    l.set_nonblocking(true)
        .map_err(|e| host_failed(IoOp::Accept, None, &e))?;
    let started = std::time::Instant::now();
    let outcome = loop {
        match l.accept() {
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= d {
                    break Err(io_fault(
                        IoOp::Accept,
                        IoKind::Timeout,
                        "no connection arrived before the deadline",
                    ));
                }
                std::thread::sleep(POLL);
            }
            other => break Ok(other),
        }
    };
    // Before the `?`, so a failed restore does not mask the accept's own
    // outcome and a successful one is not skipped by an early return.
    let restored = l
        .set_nonblocking(false)
        .map_err(|e| host_failed(IoOp::Accept, None, &e));
    outcome.and_then(|accepted| restored.map(|()| accepted))
}

/// One resolved address for `connect_timeout`, which unlike `connect` cannot
/// take a host name and try each address behind it.
fn resolve(addr: &str) -> Result<std::net::SocketAddr, Fault> {
    use std::net::ToSocketAddrs;
    addr.to_socket_addrs()
        .map_err(|e| ms_err(IoOp::Connect, addr, e))?
        .next()
        .ok_or_else(|| {
            io_fault(
                IoOp::Connect,
                IoKind::NotFound,
                "the address resolved to nothing",
            )
        })
}

/// The address goes in `:path`. ADR-042 part 1 makes that key present only when
/// the operation names a location, and for a socket the address *is* the
/// location — a `:connection-reset` with no address in it names the failure and
/// not the thing that failed.
fn ms_err(op: IoOp, addr: &str, e: std::io::Error) -> Fault {
    host_failed(op, Some(addr.to_string()), &e)
}
