int logic(int a, int b) {
    int r = 0;
    if (a > 0 && b > 0) {
        r = 1;
    }
    if (a < 0 || b < 0) {
        r = -1;
    }
    for (int i = 0; i < a; i++) {
        for (int j = 0; j < b; j++) {
            if (j == 3) {
                continue;
            }
            if (j == 5) {
                break;
            }
            r = r + 1;
        }
    }
    switch (a) {
        case 1:
            r = 10;
        case 2:
            r = 20;
            break;
    }
    return r;
}
