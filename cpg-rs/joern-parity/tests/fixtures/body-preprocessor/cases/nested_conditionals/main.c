#define ENABLE 1
int f(int x) {
#if ENABLE
#ifndef DROP
 return x;
#else
 return discarded;
#endif
#else
 return other;
#endif
}
