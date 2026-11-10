#!/usr/bin/env python3
"""One-sided Fisher exact p for the 2x2 fire table: arm-a (control) fires vs arm-b (1.9.5).
H1: fires_a > fires_b. Hypergeometric tail over tables with the same margins."""
import math, sys
from fractions import Fraction

def logC(n, k):
    return math.lgamma(n + 1) - math.lgamma(k + 1) - math.lgamma(n - k + 1)

def fisher_greater(x, n, y, m):
    # table: arm-a x/N fires, arm-b y/M fires; margins fixed
    total = n + m
    row1 = x + y                      # total fires
    p_sum = 0.0
    # possible tables: arm-a fires k ranges max(0, row1 - m) .. min(n, row1)
    lo, hi = max(0, row1 - m), min(n, row1)
    for k in range(lo, hi + 1):
        lp = logC(n, k) + logC(m, row1 - k) - logC(total, row1)
        if k >= x:
            p_sum += math.exp(lp)
    return min(1.0, p_sum)

if __name__ == "__main__":
    x, n, y, m = (int(v) for v in sys.argv[1:5])
    p = fisher_greater(x, n, y, m)
    print(f"arm-a(control): {x}/{n} fires, arm-b(1.9.5): {y}/{m} fires")
    print(f"one-sided Fisher exact p (H1: control fires more) = {p:.3e}")
    # reference P(0 fires) at observed control rate
    if x > 0:
        r = x / n
        print(f"P(0 fires in {m} | r={r:.4f}) = {math.exp(-r*m):.3e}")
