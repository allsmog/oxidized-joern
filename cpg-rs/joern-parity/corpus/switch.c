int pick(int k) {
    int r = 0;
    switch (k) {
        case 1:
            r = 10;
            break;
        case 2:
            r = 20;
            break;
        default:
            r = -1;
            break;
    }
    while (r > 0) {
        if (r == 5) {
            break;
        }
        if (r == 7) {
            continue;
        }
        r--;
    }
    return r;
}
