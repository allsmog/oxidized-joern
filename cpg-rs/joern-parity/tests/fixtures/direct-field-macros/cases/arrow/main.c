struct Item { int len; int code; };
#define Len len
int read(struct Item *p) { return p->Len; }
