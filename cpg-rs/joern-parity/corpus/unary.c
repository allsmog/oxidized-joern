int unary(int a) {
    int b = -a;
    int c = !a;
    int d = ~a;
    a++;
    --b;
    return b;
}

int ptr(int *p) {
    int v = *p;
    int *q = &v;
    return *q + v;
}
