FROM rustembedded/cross:armv7-unknown-linux-gnueabihf

RUN apt-get update
RUN dpkg --add-architecture armhf && \
    apt-get update && \
    dpkg --configure -a && \
    apt install --assume-yes \
        libssl-dev:armhf \
        libopus-dev:armhf \
        python3 \
        python3.5-dev:armhf

ENV PYO3_CROSS_INCLUDE_DIR="/usr/include"
ENV PYO3_CROSS_LIB_DIR="/usr/lib"
ENV PKG_CONFIG_LIBDIR_armv7_unknown_linux_gnueabihf=/usr/lib/arm-linux-gnueabihf/pkgconfig

