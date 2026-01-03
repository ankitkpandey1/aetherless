// SPDX-License-Identifier: Apache-2.0
// Copyright 2025 Ankit Kumar Pandey
//
// XDP Packet Redirect Program for Aetherless
//
// This eBPF program runs at the XDP hook (earliest point in network stack)
// and redirects incoming packets to function handlers based on destination port.

#include <linux/bpf.h>
#include <linux/if_ether.h>
#include <linux/ip.h>
#include <linux/tcp.h>
#include <linux/udp.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_endian.h>

// Port mapping key structure - must match Rust PortKey
struct port_key {
    __u16 port;
    __u16 _padding;
};

// Port mapping value structure - must match Rust PortValue
struct port_value {
    __u32 pid;
    __u32 addr;  // IPv4 address in network byte order
};

// BPF hash map for port-to-handler routing
// Maps destination port -> handler process info
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);
    __type(key, struct port_key);
    __type(value, struct port_value);
} port_redirect_map SEC(".maps");

// Statistics counters
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 4);
    __type(key, __u32);
    __type(value, __u64);
} stats SEC(".maps");

// Stats indices
#define STATS_PACKETS_TOTAL    0
#define STATS_PACKETS_MATCHED  1
#define STATS_PACKETS_PASSED   2
#define STATS_PACKETS_DROPPED  3

// Increment a statistics counter
static __always_inline void stats_inc(__u32 key) {
    __u64 *value = bpf_map_lookup_elem(&stats, &key);
    if (value) {
        __sync_fetch_and_add(value, 1);
    }
}

// Parse packet headers and extract destination port
// Returns destination port in host byte order, or 0 on failure
static __always_inline __u16 parse_dest_port(void *data, void *data_end) {
    // Parse Ethernet header
    struct ethhdr *eth = data;
    if ((void *)(eth + 1) > data_end) {
        return 0;
    }

    // Only handle IPv4
    if (eth->h_proto != bpf_htons(ETH_P_IP)) {
        return 0;
    }

    // Parse IP header
    struct iphdr *ip = (void *)(eth + 1);
    if ((void *)(ip + 1) > data_end) {
        return 0;
    }

    // Verify IP header length (minimum 20 bytes)
    if (ip->ihl < 5) {
        return 0;
    }

    // Calculate actual IP header size
    __u32 ip_hdr_len = ip->ihl * 4;
    void *transport = (void *)ip + ip_hdr_len;

    // Handle TCP
    if (ip->protocol == IPPROTO_TCP) {
        struct tcphdr *tcp = transport;
        if ((void *)(tcp + 1) > data_end) {
            return 0;
        }
        return bpf_ntohs(tcp->dest);
    }

    // Handle UDP
    if (ip->protocol == IPPROTO_UDP) {
        struct udphdr *udp = transport;
        if ((void *)(udp + 1) > data_end) {
            return 0;
        }
        return bpf_ntohs(udp->dest);
    }

    return 0;
}

// Main XDP program entry point
SEC("xdp")
int xdp_redirect(struct xdp_md *ctx) {
    void *data = (void *)(long)ctx->data;
    void *data_end = (void *)(long)ctx->data_end;

    stats_inc(STATS_PACKETS_TOTAL);

    // Extract destination port
    __u16 dest_port = parse_dest_port(data, data_end);
    if (dest_port == 0) {
        // Not a TCP/UDP packet or parse failed - pass to kernel
        stats_inc(STATS_PACKETS_PASSED);
        return XDP_PASS;
    }

    // Look up port in redirect map
    struct port_key key = {
        .port = dest_port,
        ._padding = 0,
    };

    struct port_value *target = bpf_map_lookup_elem(&port_redirect_map, &key);
    if (!target) {
        // Port not registered - pass to normal network stack
        stats_inc(STATS_PACKETS_PASSED);
        return XDP_PASS;
    }

    // Found a handler for this port!
    // In a full implementation, we would redirect to the handler's socket.
    // For now, we just pass the packet - the handler is already listening.
    //
    // Future enhancement: Use bpf_sk_redirect_map() or bpf_redirect()
    // to directly send to the handler socket, bypassing kernel TCP/IP stack.
    
    stats_inc(STATS_PACKETS_MATCHED);
    
    // Log the match (will appear in /sys/kernel/debug/tracing/trace_pipe)
    bpf_printk("aetherless: port %d -> pid %d", dest_port, target->pid);

    return XDP_PASS;
}

// XDP program for dropping unregistered traffic (optional strict mode)
SEC("xdp/strict")
int xdp_redirect_strict(struct xdp_md *ctx) {
    void *data = (void *)(long)ctx->data;
    void *data_end = (void *)(long)ctx->data_end;

    stats_inc(STATS_PACKETS_TOTAL);

    __u16 dest_port = parse_dest_port(data, data_end);
    if (dest_port == 0) {
        stats_inc(STATS_PACKETS_PASSED);
        return XDP_PASS;
    }

    struct port_key key = {
        .port = dest_port,
        ._padding = 0,
    };

    struct port_value *target = bpf_map_lookup_elem(&port_redirect_map, &key);
    if (!target) {
        // In strict mode, drop packets to unregistered ports
        stats_inc(STATS_PACKETS_DROPPED);
        return XDP_DROP;
    }

    stats_inc(STATS_PACKETS_MATCHED);
    return XDP_PASS;
}

char _license[] SEC("license") = "Apache-2.0";
