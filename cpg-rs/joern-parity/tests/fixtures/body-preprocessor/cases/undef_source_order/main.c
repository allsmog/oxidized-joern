#define FLAG 1
#ifdef FLAG
int before(int x) {
#if 1
 return x;
#else
 return 0;
#endif
}
#endif
#  undef FLAG
#ifdef FLAG
int after(int x) {
#if 1
 return x;
#else
 return 0;
#endif
}
#endif
