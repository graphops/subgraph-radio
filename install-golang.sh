if [ $(uname -m) = "aarch64" ]; then
    wget https://golang.org/dl/go1.19.5.linux-arm64.tar.gz &&
        tar -C /usr/local -xzf go1.19.5.linux-arm64.tar.gz
else
    wget https://golang.org/dl/go1.19.5.linux-amd64.tar.gz &&
        tar -C /usr/local -xzf go1.19.5.linux-amd64.tar.gz
fi
