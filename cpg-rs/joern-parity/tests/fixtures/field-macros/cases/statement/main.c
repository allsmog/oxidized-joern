struct Item { int len; int code; };
#define Len len
#define SET(p, v) do { (p)->Len = (v); } while (0)
void write(struct Item *p, int value) { SET(p, value); }
