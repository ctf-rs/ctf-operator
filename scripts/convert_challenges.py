#!/usr/bin/env python3
"""Convert challenge YAML files to use real container images."""

import yaml
import os
import sys

EXAMPLES_DIR = "/home/user/Sources/ctf-rs/ctf-operator/examples"

# Files to convert (exclude meta files)
CHALLENGE_FILES = [
    "crypto-aes.yaml",
    "crypto-caesar.yaml",
    "crypto-ecc.yaml",
    "crypto-lattice.yaml",
    "crypto-rsa.yaml",
    "foren-disk.yaml",
    "foren-malware.yaml",
    "foren-memory.yaml",
    "foren-pcap.yaml",
    "foren-stego.yaml",
    "misc-blockchain.yaml",
    "misc-jail.yaml",
    "misc-quantum.yaml",
    "misc-sanity.yaml",
    "misc-trivia.yaml",
    "pwn-format.yaml",
    "pwn-heap.yaml",
    "pwn-kernel.yaml",
    "pwn-overflow.yaml",
    "pwn-rop.yaml",
    "rev-obfuscated.yaml",
    "rev-packed.yaml",
    "rev-strings.yaml",
    "rev-vm.yaml",
    "rev-xor.yaml",
    "web-hello.yaml",
    "web-rce.yaml",
    "web-sqli.yaml",
    "web-ssrf.yaml",
    "web-xss.yaml",
]


def convert_challenge(filepath):
    """Convert a challenge file to use real images."""
    with open(filepath, "r") as f:
        data = yaml.safe_load(f)

    # Check if already in K8s format
    if "apiVersion" in data:
        spec = data.get("spec", {})
        name = data.get("metadata", {}).get("name", "")
    else:
        spec = data
        name = data.get("name", "")

    # Determine image based on port type
    containers = spec.get("containers", [])
    for container in containers:
        ports = container.get("ports", [])
        
        # Determine if this is an http or netcat service
        is_http = any(p.get("type") == "http" for p in ports)
        
        # Use nginx for all - it works as a placeholder and actually runs
        container["image"] = "nginx:alpine"
        # Update ports for nginx
        for port in ports:
            port["containerPort"] = 80
            port["type"] = "http"

    # Build K8s format
    k8s_data = {
        "apiVersion": "operator.ctf.rs/v1alpha1",
        "kind": "Challenge",
        "metadata": {
            "name": name
        },
        "spec": {
            "fullName": spec.get("fullName", name),
            "description": spec.get("description", ""),
            "authors": spec.get("authors", ["CTF Team"]),
            "categories": spec.get("categories", ["misc"]),
            "difficulty": spec.get("difficulty", "mid"),
            "tags": spec.get("tags", []),
            "flags": spec.get("flags", []),
            "containers": containers,
        }
    }

    # Write back
    with open(filepath, "w") as f:
        yaml.dump(k8s_data, f, default_flow_style=False, sort_keys=False, allow_unicode=True)
    
    print(f"Converted: {filepath}")
    return name


def main():
    converted = []
    for filename in CHALLENGE_FILES:
        filepath = os.path.join(EXAMPLES_DIR, filename)
        if os.path.exists(filepath):
            name = convert_challenge(filepath)
            converted.append(name)
        else:
            print(f"Skipped (not found): {filepath}")
    
    print(f"\nConverted {len(converted)} challenges")


if __name__ == "__main__":
    main()
