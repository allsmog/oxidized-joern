#define LIMIT 10
#define SQR(x) ((x) * (x))
#define UNUSED_FLAG 1

int macros(int n) {
    if (n > LIMIT) {
        return SQR(n);
    }
#ifdef UNUSED_FLAG
    n = n + LIMIT;
#endif
#ifdef NOT_DEFINED
    n = 999;
#endif
    return 0;
}
