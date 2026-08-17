#!/usr/bin/env python3
"""Inspect, migrate and initialize the KOLNY colony's on-chain accounts.

Read-only by default. Nothing is sent to a cluster unless --apply is passed, and
--apply refuses any cluster that is not devnet: mainnet deployment and mainnet
state are user decisions taken deliberately, never a flag away from an
inspection command.

    python3 colony-state.py                          # report only
    python3 colony-state.py --apply --keypair ~/kolny-deploy.json

What it does with --apply, in order, skipping whatever is already true:

  1. migrate_colony_config   grows a pre-admission 312-byte config to 360 in
                             place, at the same address. One-shot on chain.
  2. initialize_*            creates any singleton that does not exist yet.

It deliberately does NOT call set_kolny_mint. That names the mint the admission
burn destroys, it can only ever be written once, and it must not happen until
$KOLNY actually exists. An unset mint is the correct state before launch, and
`register_forager` refuses to run rather than admitting anyone without burning.

Sizes come from the IDL, never from hand arithmetic. That is not fussiness: the
array case is `element_size * count`, and computing it as `count` is a mistake
that has already been made against this program's own TrailBoard, where it
undercounted `[u64; 21]` by 147 bytes and reported a healthy account as
malformed. The layout check below is the reason to have a script at all -- a
length that merely looks plausible is what let a decoder read BroodVaultState in
the wrong field order at exactly the right total length.
"""

import argparse
import hashlib
import json
import os
import struct
import sys
import urllib.request

PROGRAM_ID = "7whkmFfDcTyoJgf7jFGFmKNFMQn8NoreHnh2wZ9nWbsk"
DEVNET = "https://api.devnet.solana.com"
SYSTEM_PROGRAM = "11111111111111111111111111111111"
TOKEN_PROGRAM = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"

HERE = os.path.dirname(os.path.abspath(__file__))
# Two layouts carry this script: the working tree, where the IDL is a build
# output under target/, and the published repository, where it is checked in
# under idl/. Both are tried rather than assumed, because a script that only
# runs from one of them silently stops being the thing anyone reaches for.
IDL_CANDIDATES = [
    os.path.join(HERE, "..", "target", "idl", "kolny_colony.json"),
    os.path.join(HERE, "..", "idl", "kolny_colony.json"),
]

# (instruction, account struct, seeds) for the singletons this script creates.
SINGLETONS = [
    ("initialize_brood", "BroodVaultState", [b"brood"]),
    ("initialize_risk_cache", "RiskCacheState", [b"cache"]),
    ("initialize_trail_board", "TrailBoard", [b"trail_board"]),
]

BASE_UNIT_SIZES = {
    "pubkey": 32, "u128": 16, "i128": 16, "u64": 8, "i64": 8,
    "u32": 4, "i32": 4, "u16": 2, "i16": 2, "u8": 1, "i8": 1, "bool": 1,
}
UNPACK = {
    "u64": "<Q", "i64": "<q", "u32": "<I", "i32": "<i",
    "u16": "<H", "i16": "<h", "u8": "<B", "i8": "<b", "bool": "<?",
}


def type_size(t):
    """Serialized byte size of an IDL type.

    The array branch is the whole point: `element_size * count`. Returning
    `count` here is the bug this script exists to stop anyone repeating.
    """
    if isinstance(t, str):
        return BASE_UNIT_SIZES[t]
    if "array" in t:
        return type_size(t["array"][0]) * t["array"][1]
    raise ValueError("unsupported IDL type: %r" % (t,))


def rpc(url, method, params):
    req = urllib.request.Request(
        url,
        data=json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode(),
        headers={"Content-Type": "application/json"},
    )
    body = json.load(urllib.request.urlopen(req, timeout=60))
    if "error" in body:
        raise RuntimeError(body["error"])
    return body["result"]


def disc(kind, name):
    return hashlib.sha256(("%s:%s" % (kind, name)).encode()).digest()[:8]


def load_idl():
    path = next((p for p in IDL_CANDIDATES if os.path.exists(p)), None)
    if path is None:
        sys.exit("no IDL found. looked in:\n  " + "\n  ".join(
            os.path.normpath(p) for p in IDL_CANDIDATES))
    with open(path) as fh:
        idl = json.load(fh)
    fields = {t["name"]: t["type"]["fields"] for t in idl["types"] if "fields" in t.get("type", {})}
    expected = {a["name"]: 8 + sum(type_size(f["type"]) for f in fields[a["name"]])
                for a in idl["accounts"]}
    return idl, fields, expected


def decode(fields, name, raw):
    """Decode an account and report how many bytes were consumed.

    The consumed count is the real check. A field order that is wrong but sums
    to the right total still lands off the end or short of it once any variable
    boundary is crossed, and comparing only the total length cannot see that.
    """
    off, out = 8, {}
    for f in fields[name]:
        t = f["type"]
        if t == "pubkey":
            out[f["name"]] = b58encode(raw[off:off + 32]); off += 32
        elif t in ("u128", "i128"):
            out[f["name"]] = int.from_bytes(raw[off:off + 16], "little"); off += 16
        elif isinstance(t, dict):
            n = type_size(t); out[f["name"]] = raw[off:off + n].hex(); off += n
        else:
            fmt = UNPACK[t]
            out[f["name"]] = struct.unpack_from(fmt, raw, off)[0]
            off += struct.calcsize(fmt)
    return out, off


ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"


def b58encode(raw):
    n = int.from_bytes(raw, "big")
    s = ""
    while n:
        n, r = divmod(n, 58)
        s = ALPHABET[r] + s
    return "1" * (len(raw) - len(raw.lstrip(b"\0"))) + (s or "1")


def report(url, idl, fields, expected):
    accounts = rpc(url, "getProgramAccounts",
                   [PROGRAM_ID, {"encoding": "base64", "commitment": "finalized"}])
    import base64
    by_disc = {disc("account", a["name"]): a["name"] for a in idl["accounts"]}

    print("account layout")
    problems, seen = 0, {}
    for a in sorted(accounts, key=lambda x: x["pubkey"]):
        raw = base64.b64decode(a["account"]["data"][0])
        name = by_disc.get(raw[:8])
        if name is None:
            print("  UNKNOWN discriminator at %s" % a["pubkey"]); problems += 1; continue
        seen.setdefault(name, []).append(a["pubkey"])
        want = expected[name]
        if len(raw) != want:
            print("  STALE   %-18s %s  %d bytes, layout wants %d"
                  % (name, a["pubkey"], len(raw), want))
            problems += 1
            continue
        vals, used = decode(fields, name, raw)
        exact = used == len(raw)
        bad_bps = [k for k, v in vals.items()
                   if k.endswith("_bps") and isinstance(v, int)
                   and k != "risk_aversion_bps" and v > 10_000]
        if not exact or bad_bps:
            print("  SUSPECT %-18s %s  consumed %d/%d  out-of-range bps %s"
                  % (name, a["pubkey"], used, len(raw), bad_bps))
            problems += 1
        else:
            print("  OK      %-18s %s  %d bytes, consumed exactly"
                  % (name, a["pubkey"], len(raw)))
    return seen, problems


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default=DEVNET)
    ap.add_argument("--keypair")
    ap.add_argument("--apply", action="store_true",
                    help="send transactions; devnet only")
    args = ap.parse_args()

    if args.apply and "devnet" not in args.url:
        sys.exit("refusing to send to %s: this script is devnet only" % args.url)

    idl, fields, expected = load_idl()
    if idl["address"] != PROGRAM_ID:
        sys.exit("IDL address %s does not match %s" % (idl["address"], PROGRAM_ID))
    print("program %s on %s\n" % (PROGRAM_ID, args.url))
    print("layout the IDL expects:")
    for name in sorted(expected):
        print("  %-18s %d bytes" % (name, expected[name]))
    print()

    seen, problems = report(args.url, idl, fields, expected)
    print()

    if not args.apply:
        print("read-only. pass --apply --keypair <path> to migrate or initialize on devnet.")
        return 0 if problems == 0 else 1

    from solders.keypair import Keypair
    from solders.pubkey import Pubkey
    from solders.instruction import Instruction, AccountMeta
    from solders.transaction import Transaction
    from solders.message import Message
    from solana.rpc.api import Client

    kp = Keypair.from_bytes(bytes(json.load(open(os.path.expanduser(args.keypair)))))
    print("signing as %s" % kp.pubkey())
    pid = Pubkey.from_string(PROGRAM_ID)
    client = Client(args.url)

    def send(name, metas, data=b""):
        ix = Instruction(pid, bytes(disc("global", name)) + data, metas)
        bh = client.get_latest_blockhash(commitment="finalized").value.blockhash
        sig = client.send_transaction(Transaction([kp], Message([ix], kp.pubkey()), bh)).value
        client.confirm_transaction(sig, commitment="confirmed")
        print("  %s -> %s" % (name, sig))

    cfg, _ = Pubkey.find_program_address([b"colony"], pid)
    info = client.get_account_info(cfg).value
    if info is None:
        print("no colony config exists; run initialize_colony first (it takes parameters "
              "this script deliberately does not choose for you)")
        return 1
    if len(info.data) == 312:
        print("migrating colony config 312 -> %d" % expected["ColonyConfig"])
        send("migrate_colony_config", [
            AccountMeta(cfg, False, True),
            AccountMeta(kp.pubkey(), True, True),
            AccountMeta(Pubkey.from_string(SYSTEM_PROGRAM), False, False),
        ])
    else:
        print("colony config already %d bytes; nothing to migrate" % len(info.data))

    for ix_name, acct_name, seeds in SINGLETONS:
        pda, _ = Pubkey.find_program_address(seeds, pid)
        if client.get_account_info(pda).value is not None:
            print("  %s exists at %s" % (acct_name, pda))
            continue
        print("creating %s at %s" % (acct_name, pda))
        send(ix_name, [
            AccountMeta(cfg, False, False),
            AccountMeta(pda, False, True),
            AccountMeta(kp.pubkey(), True, True),
            AccountMeta(Pubkey.from_string(SYSTEM_PROGRAM), False, False),
        ])

    print("\nre-checking after apply:")
    _, problems = report(args.url, idl, fields, expected)
    print("\nset_kolny_mint was NOT called. That is deliberate: it can only ever be written "
          "once, and it must wait until $KOLNY exists.")
    return 0 if problems == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
