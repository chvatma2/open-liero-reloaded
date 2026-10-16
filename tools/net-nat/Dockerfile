ARG BASE
FROM ${BASE}
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends iproute2 nftables miniupnpd-nftables \
    && rm -rf /var/lib/apt/lists/*
COPY router.sh node.sh miniupnpd.conf.in /olr-nat/
RUN chmod 755 /olr-nat/*.sh
