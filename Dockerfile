# syntax=docker/dockerfile:1

FROM rust:1.88-slim AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src-rs ./src-rs

RUN cargo build --locked --release --bin ghost-api-server

FROM debian:bookworm-slim AS runtime
WORKDIR /app

ARG MUPDF_VERSION=1.27.1
ARG MUPDF_MIN_VERSION=1.26.8

RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    ca-certificates \
    build-essential \
    curl \
    git \
    ghostscript \
    libfreetype-dev \
    libharfbuzz-dev \
    libjbig2dec0-dev \
    libjpeg-dev \
    libopenjp2-7-dev \
    pkg-config \
    poppler-utils \
    python3 \
    zlib1g-dev \
  && git clone --depth 1 --branch "${MUPDF_VERSION}" --recurse-submodules \
    https://github.com/ArtifexSoftware/mupdf.git "/tmp/mupdf-${MUPDF_VERSION}" \
  && make -C "/tmp/mupdf-${MUPDF_VERSION}" HAVE_X11=no HAVE_GLUT=no prefix=/usr/local install \
  && INSTALLED_MUTOOL_VERSION="$(mutool 2>&1 | awk '/^mutool version/{print $3; exit}')" \
  && test -n "${INSTALLED_MUTOOL_VERSION}" \
  && printf '%s\n%s\n' "${MUPDF_MIN_VERSION}" "${INSTALLED_MUTOOL_VERSION}" | sort -V -C \
  && mutool 2>&1 | grep -Eq "recolor[[:space:]]+--" \
  && rm -rf "/tmp/mupdf-${MUPDF_VERSION}" \
  && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/ghost-api-server /usr/local/bin/ghost-api-server

EXPOSE 9001

CMD ["/usr/local/bin/ghost-api-server"]
