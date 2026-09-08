struct Item { int len; int code; };
#define Len len
#define READ(p) ((p)->code)
int read(struct Item *p) { return READ(p); }
