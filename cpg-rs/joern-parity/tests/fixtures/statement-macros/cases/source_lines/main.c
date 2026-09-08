#define SET(p,v) do { int *slot=(p); *slot=(v); } while(0)
void sink(int v);
void z_first(int *p, int v) {
  SET(p,v);
  sink(v);
}
void a_second(int *p, int v) {
  SET(p,v);
  sink(v);
}
