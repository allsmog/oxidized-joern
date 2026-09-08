#define FLAG 1
#if 1
#undef FLAG
#endif
#if defined(FLAG)
int hidden(int x) {
#if 1
 return x;
#else
 return 0;
#endif
}
#endif
int visible(int x) { return x; }
