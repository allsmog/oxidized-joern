const int identity(const int value) { const int local = value; return local; }
const int declared(const int value);
const int call_definition(const int value) { return identity(value); }
const int call_prototype(const int value) { return declared(value); }
