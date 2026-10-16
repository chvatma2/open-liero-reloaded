# OpenLieroX at the commit pinned in content/olx-manifest.ron, plus the
# OLR oracle test hooks (oracle-hooks.patch), built as a headless
# dedicated server. `cargo xtask oracle-record` builds and runs this image.
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake git ca-certificates python3 \
        libsdl2-dev libsdl2-image-dev libsdl2-mixer-dev libxml2-dev libgd-dev \
        zlib1g-dev libzip-dev libx11-dev libcurl4-openssl-dev libboost-dev \
        libopenal-dev libalut-dev libvorbis-dev libyaml-cpp-dev \
        binutils-dev libiberty-dev \
    && rm -rf /var/lib/apt/lists/*

ARG OLX_COMMIT=d1ad953f4f20610f2648cce26c9739e7c088409d
RUN git init -q /olx && cd /olx \
    && git remote add origin https://github.com/openlierox/openlierox.git \
    && git fetch -q --depth 1 origin "$OLX_COMMIT" && git checkout -q FETCH_HEAD

COPY oracle-hooks.patch /olx/
RUN cd /olx && git apply oracle-hooks.patch \
    && cmake -DDEDICATED_ONLY=Yes -DHAWKNL_BUILTIN=Yes -DOLX_DOWNLOAD_GAMECONTROLLERDB=OFF -DDEBUG=No . \
    && make -j"$(nproc)"

COPY oracle_control.py /olx/oracle_control.py
RUN chmod 755 /olx/oracle_control.py
WORKDIR /olx/share/gamedir
