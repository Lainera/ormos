REGISTRY ?= ''
BIN = rpx
FEATURES = instrument
ifeq ($(REGISTRY), '')
	TAG ?= $(BIN)
else
	TAG ?= $(REGISTRY)/$(BIN)
endif

makefile_dir := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))

vendor: Cargo.toml Cargo.lock
	cp .cargo/config.toml .cargo/vendor && cargo vendor >> .cargo/vendor

image: vendor 
	docker buildx build . \
		-t "$(TAG):arm64v8" \
		--platform=linux/arm64/v8 \
		--build-arg target=armv7-unknown-linux-musleabihf \
		--build-arg bin=$(BIN) \
		--build-arg features=$(FEATURES) \
		--push

binary: image 
	docker run -it --rm -v $(makefile_dir):/tmp/export --entrypoint cp "$(TAG):arm64v8" /$(BIN) /tmp/export/$(BIN)

clean:
	rm rpx 2> /dev/null; rm pipe 2> /dev/null; cargo clean
