signed int identity(signed int value) { signed int local = value; return local; }
signed int declared(signed int value);
signed int call_definition(signed int value) { return identity(value); }
signed int call_prototype(signed int value) { return declared(value); }
