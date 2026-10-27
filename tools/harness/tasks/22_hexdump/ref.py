"""The reference solution of 22_hexdump: it defines the expected outputs
(tests/), and is never shown to a model."""
import random

def solve(inp):
    data = inp.encode("latin-1")
    out = []
    for off in range(0, len(data), 16):
        chunk = data[off:off + 16]
        hexs = " ".join(f"{b:02x}" for b in chunk).ljust(47)
        text = "".join(chr(b) if 32 <= b <= 126 else "." for b in chunk)
        out.append(f"{off:08x}  {hexs}  |{text}|")
    out.append(f"{len(data):08x}")
    return ''.join(o + "\n" for o in out)

def cases(rng):
    out = ["Hello, world!\n", "", "0123456789abcdef", "0123456789abcdef\n"]
    for n in [100, 3000]:
        # text: tabs and newlines among printable characters (no carriage
        # returns, which a language may translate on reading)
        out.append("".join(chr(rng.choice([rng.randint(32, 126), 9, 10])) for _ in range(n)))
    return out
