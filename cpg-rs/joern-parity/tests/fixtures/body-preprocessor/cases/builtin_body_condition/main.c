int f(int x) {
#if __STDC_VERSION__ >= 199901L
 return x;
#else
 return discarded;
#endif
}
