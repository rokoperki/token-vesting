# Multi-Token Vesting Contract

A smart contract for managing step-based vesting of multiple tokens using reusable vesting schedules. It enables consistent token distribution with cliffs, deterministic release over time, and secure claiming by recipients.

## Overview

This contract is designed for token distributions to teams, advisors, investors, or other groups that require standardized vesting terms. Each token can have multiple vesting schedules, and each schedule can be reused by multiple recipients.

The system ensures tokens are fully locked at allocation time and can only be claimed as they vest over time.

## Core Instructions

### Initialize Schedule

Creates a vesting schedule for a specific token.

Each schedule defines:
- Token mint
- Start time
- Cliff duration
- Total vesting duration
- Step duration

No tokens are claimable before the cliff. After the cliff, tokens vest in discrete steps until fully vested.

### Add Participant

Adds a recipient to an existing vesting schedule with a fixed token allocation.

- Tokens are locked and reserved for vesting
- Multiple recipients may share the same schedule
- Each allocation is tracked independently

### Claim Tokens

Allows a recipient to claim vested tokens from their allocation.

- Only the recipient may claim
- Claims can be made multiple times
- Only vested and unclaimed tokens are released
- Claims before the cliff release zero tokens
- After full vesting, all remaining tokens can be claimed

## Safety Guarantees

- No early token claims
- No double claiming
- Deterministic vesting based on time
- All allocations are fully backed by locked tokens
- Independent tracking per token, schedule, and allocation

## Build & Test

```bash
# Build the program
cargo build-spf

# Run tests
cargo test
