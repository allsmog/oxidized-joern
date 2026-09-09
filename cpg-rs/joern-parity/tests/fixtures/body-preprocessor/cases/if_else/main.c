int first(int x);
int second(int x);
int choose(int x) {
#if 0
    return first(x);
#else
    return second(x);
#endif
}
