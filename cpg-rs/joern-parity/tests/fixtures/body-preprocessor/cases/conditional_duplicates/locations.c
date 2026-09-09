#define API
API
int same(int x) {
  return x;
}
#if 0
API
int same(int x) {
  return x + 1;
}
#else
API
int same(int x) {
  return x + 2;
}
#endif
