#define FLAG 2
int f(int x) {
#ifdef MISSING
 return discarded;
#elif FLAG == 2
 return x;
#else
 return other;
#endif
}
