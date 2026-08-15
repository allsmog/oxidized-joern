#include <cstdio>
#include <string>

static std::string source(std::string value) {
  return value;
}

static std::string transform(std::string value) {
  return value;
}

static void sink(std::string value) {
  std::puts(value.c_str());
}

int main(std::string user) {
  auto raw = source(user);
  auto clean = transform(raw);
  sink(clean);
  return 0;
}
