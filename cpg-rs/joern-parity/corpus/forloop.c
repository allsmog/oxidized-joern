int count(int n) {
    int t = 0;
    for (int i = 0; i < n; i++) {
        t = t + i;
    }
    do {
        t--;
    } while (t > 100);
    int m = t > 0 ? t : 0;
    return m;
}
