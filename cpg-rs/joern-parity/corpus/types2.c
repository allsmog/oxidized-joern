typedef unsigned int uint_t;

enum color { RED, GREEN = 5, BLUE };

union value {
    int i;
    float f;
};

static int counter;

int apply(int (*fn)(int, int), int x) {
    return fn(x, x);
}

int use(int n) {
    uint_t u = (uint_t)n;
    enum color c = GREEN;
    union value v;
    v.i = n;
    int grid[2][3];
    grid[1][2] = n;
    return c + v.i;
}
