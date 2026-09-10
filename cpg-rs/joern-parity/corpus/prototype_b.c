int shared(int other);
int only_header(double value);
int use_b(int x) { return shared(x); }
int shared(int value) { return value; }
