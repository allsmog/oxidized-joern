struct point {
    int x;
    int y;
};

int gcounter;

int helper(int v);

int agg(struct point *p, int vals[]) {
    struct point q;
    q.x = vals[0];
    q.y = p->y;
    int a, b = 1;
    a = q.x + b;
    gcounter = a;
    return helper(a);
}

int helper(int v) {
    return v + gcounter;
}
