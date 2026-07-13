int gotos(int n) {
    int r = 0;
loop:
    r = r + n;
    n = n - 1;
    if (n > 0) {
        goto loop;
    }
    r = r * 2;
done:
    return r;
}
