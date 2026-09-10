struct Item { int values[4]; int len; };
#define Len values[1]
void use(int x);
int read(struct Item *p) {
  int first = p->Len;
  use(first);
  int second = p->Len;
  use(second);
  return second;
}
