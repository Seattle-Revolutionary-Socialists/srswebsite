FROM node:26-slim

ARG WORKER_BUILD_VERSION=0.8.5
ARG RUST_VERSION=1.97.1
ENV WORKER_BUILD_VERSION="${WORKER_BUILD_VERSION}"


RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential pkg-config libssl-dev \
    python3 python3-pip dpkg-dev pandoc texlive-latex-base texlive-latex-recommended

RUN npm i -g pnpm@latest

RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --default-toolchain "${RUST_VERSION}"
ENV PATH="/root/.cargo/bin:${PATH}"

RUN npm install -g @tailwindcss/cli

RUN curl -fsSL -o /tmp/zola.deb  "https://github.com/barnumbirr/zola-debian/releases/download/v0.22.1-1/zola_0.22.1-1_amd64_trixie.deb" 
RUN dpkg -i /tmp/zola.deb
RUN rm -f /tmp/zola.deb

RUN set -eux; \
    MULTIARCH="$(dpkg-architecture -qDEB_HOST_MULTIARCH)"; \
    export PKG_CONFIG_PATH="/usr/lib/${MULTIARCH}/pkgconfig:/usr/lib/pkgconfig:/usr/share/pkgconfig"; \
    test -f "/usr/lib/${MULTIARCH}/pkgconfig/openssl.pc"; \
    pkg-config --modversion openssl; \
    cargo install \
        --locked \
        --version "${WORKER_BUILD_VERSION}" \
        worker-build; \
    worker-build --version

RUN apt-get install -y just

RUN curl https://wasm-bindgen.github.io/wasm-pack/installer/init.sh -sSf | sh

WORKDIR /work

CMD ["bash"]
