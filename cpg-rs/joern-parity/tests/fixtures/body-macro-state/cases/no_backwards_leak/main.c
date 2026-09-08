int before(int x) {
#if LATE
 return x;
#else
 return 0;
#endif
}
void setup(void) {
#define LATE 1
}
int after(int x) {
#if LATE
 return x;
#else
 return 0;
#endif
}
