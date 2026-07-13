package demo

import kotlin.math.max

data class User(val name: String, val age: Int) {
  val label: String = "$name:$age"

  fun score(flag: Boolean): Int {
    val base = if (flag) age else max(age - 1, 0)
    return when {
      base > 18 -> base
      else -> 0
    }
  }
}

fun transform(values: List<Int>): List<Int> =
  values.map { value -> value + 1 }
