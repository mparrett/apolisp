# Runs the same gate on Linux that `just verify` runs on macOS.
#
# BUILD.md files this as a follow-up, and the reason is not portability as a
# goal. It is that a platform-specific assumption baked into a golden file or a
# `:kind` mapping is invisible on the machine that wrote it. TRAPS.md already
# records a read deadline raising `:would-block` on Unix and `:timeout` on
# Windows; the same class of divergence is available between macOS and Linux
# for socket errors, terminal capability detection, and path handling, and
# through milestone 10 none of it was exercised.
#
# Tooling, so outside the line budget (BUILD.md).
#
# This catches OS divergence, not architecture divergence: on an arm64 host the
# daemon runs arm64 Linux natively. For the other axis, pass
# `--platform linux/amd64` to the build — it is emulated and slow, and nothing
# has yet suggested the project has an arch-sensitive assumption.

# Pinned to the toolchain the host verifies with. A floating tag would make
# this gate's answer depend on the day it ran, and determinism is a
# prerequisite here (BUILD.md) rather than a preference. Bumping it is a
# reviewed diff.
FROM rust:1.97.1-slim AS gate

# `just` is the one command anyone runs, so the container runs the real recipes
# rather than a hand-copied list that drifts from the justfile the first time
# one changes. Debian trixie packages it; `cargo install just` would buy a
# from-source build for a version difference no recipe here uses.
RUN apt-get update \
 && apt-get install --yes --no-install-recommends just \
 && rm -rf /var/lib/apt/lists/*

# `just verify` is fmt-check and clippy before it is anything else, and the
# base image installs the toolchain with the minimal profile. Note that
# `which cargo-fmt` finds a hit either way — `/usr/local/cargo/bin` holds
# rustup *proxies*, which exist for components that are not installed and fail
# only when run.
RUN rustup component add rustfmt clippy

WORKDIR /apolisp

# COPY rather than a bind mount, on purpose. "Does this pass on Linux" must not
# depend on what is lying around in the host working directory, and the host's
# `target/` holds Darwin artifacts that would be silently wrong here.
# `.dockerignore` keeps both out of the context.
COPY . .

# Exactly `just verify` and nothing more. If the container ran a rung the host
# gate does not, a red container would be ambiguous between "Linux differs" and
# "this rung was never in the gate" — which is the one question this image
# exists to answer unambiguously.
CMD ["just", "verify"]

# --- soak -------------------------------------------------------------------
#
# BUILD.md's ladder ends `merge → soak → tag`, and the soak's leak leg needs a
# tool the gate has no use for. A separate stage (`--target soak`) so the gate
# image stays exactly what its CMD claims — an image carrying a profiler is an
# image someone eventually profiles in, and then the gate is not the gate.
FROM gate AS soak

RUN apt-get update \
 && apt-get install --yes --no-install-recommends valgrind \
 && rm -rf /var/lib/apt/lists/*

CMD ["sh", "soak.sh"]
