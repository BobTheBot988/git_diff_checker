# Stage 1: Fetch Foundry binaries
FROM ghcr.io/foundry-rs/foundry:sha256-5a2a275f7ac9c2e752bac64d4ac6a7ce26fe498d5f35185ab95a46c21e2bc71a AS foundry-binaries

# Stage 2: Final Alpine Image
FROM alpine:3.23.4

# Install runtime and build dependencies
# We need python3, pip, and libstdc++ for Z3. 
# gcc, g++, python3-dev, and musl-dev are included to build wheel dependencies if needed.
RUN apk add --no-cache \
  libstdc++~=14 \
  git~=2 \
  nodejs~=22 \
  npm~=10 \
  python3~=3.12 \
  py3-pip~=24 \
  curl~=8 \
  bash~=5 \
  gcc~=14 \
  g++~=14 \
  python3-dev~=3.12 \
  musl-dev~=1.2 \
  linux-headers~=6

# Copy Foundry binaries from the first stage
COPY --from=foundry-binaries /usr/local/bin/forge /usr/local/bin/forge
COPY --from=foundry-binaries /usr/local/bin/cast /usr/local/bin/cast
COPY --from=foundry-binaries /usr/local/bin/anvil /usr/local/bin/anvil
COPY --from=foundry-binaries /usr/local/bin/chisel /usr/local/bin/chisel

# Set up a Python Virtual Environment to avoid PEP 668 externally-managed environment errors
ENV VIRTUAL_ENV=/opt/venv
RUN python3 -m venv $VIRTUAL_ENV
ENV PATH="$VIRTUAL_ENV/bin:$PATH"

# Upgrade pip and install Halmos along with its SMT solver requirements
RUN pip install --no-cache-dir --upgrade pip setuptools=61.0 wheel=0.42.0 && \
  pip install --no-cache-dir halmos= 0.1.0

# Clean up build dependencies to keep the image lightweight
RUN apk del gcc g++ python3-dev musl-dev linux-headers

# Verify installations
RUN forge --version && halmos --help

WORKDIR /workspace

CMD ["/bin/bash"]
