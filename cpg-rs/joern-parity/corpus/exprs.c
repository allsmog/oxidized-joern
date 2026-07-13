int exprs(int a, int b) {
    a += b;
    a -= 2;
    a *= b;
    a /= 2;
    a %= 3;
    a <<= 1;
    a >>= 1;
    a &= b;
    a |= b;
    a ^= b;
    long w = (long)a;
    unsigned long s = sizeof(int);
    double f = 1.5;
    char c = 'x';
    const char *msg = "hi";
    a = (b++, b + 1);
    return a;
}
