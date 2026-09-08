struct Item { int len; int code; };
#define Len len
#define PTR(p) (p)
int read(struct Item *p) { return PTR(p)->Len; }
