struct Item { int len; int width; int values[4]; };
int fn(int index);
#define Len values[fn(index)]
int f(struct Item *p, int index) { return p->Len; }
