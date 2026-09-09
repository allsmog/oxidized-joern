#define FLAG 1
int before(int x) {
#if FLAG
    return x;
#else
    return 0;
#endif
}
#undef FLAG
#define FLAG 0
int after(int x) {
#if FLAG
    return x;
#else
    return 0;
#endif
}
