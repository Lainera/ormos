FROM --platform=$BUILDPLATFORM rust:1-slim-buster as builder
LABEL stage=builder 

ARG target
ARG bin 
ARG features=default

WORKDIR /code
RUN rustup target add ${target} 
RUN apt-get -yqq update 
RUN apt-get -yqq install \ 
	ca-certificates \
	binutils-arm-linux-gnueabihf \
	gcc-arm-linux-gnueabihf

COPY ./Cargo.* ./ 
COPY ./.cargo/vendor ./.cargo/config.toml
COPY ./src ./src 
COPY ./vendor ./vendor 

ENV CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER=arm-linux-gnueabihf-ld
ENV TARGET_CC=arm-linux-gnueabihf-gcc
ENV TARGET_AR=arm-linux-gnueabihf-gcc-ar

RUN cargo build \
	--offline \
	--release \
	--bin ${bin} \
	--features ${features} \
	--target ${target} 

FROM debian:buster-slim 
ARG target 
ARG bin 

COPY --from=builder /code/target/${target}/release/${bin} /${bin}
CMD /${bin}
