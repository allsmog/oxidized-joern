int classify(int n) {
    int r = 0;
    if (n > 10) {
        r = n - 1;
    } else {
        r = n * 2;
    }
    return r;
}
