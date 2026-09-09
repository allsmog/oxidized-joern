int choose(int base, int x) {
 int result;
#if !defined(USE_OLD)
 if (base == 2)
  result = x;
 else
#endif
 if (base == 10)
  result = 10;
 else
  result = 0;
 return result;
}
