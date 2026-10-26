package io.github.smallgearlab.sortition

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

/** The library has no Kotlin of its own: this checks that it reads naturally from Kotlin. */
class KotlinTest {
    @Test
    fun `the quickstart gives the same winners as every other binding`() {
        val people = listOf("alice", "bob", "carol", "dave")
        val winners = Lottery.drawItems(Items.of(people), 2, Lottery.seedFromText("my seed"))
        assertEquals(listOf("dave", "bob"), winners.map { people[it.index] })
    }

    @Test
    fun `sessions close with use and errors carry the code`() {
        Session.create(10, 1L).use { session ->
            val first = session.draw(3, "request-1")
            assertEquals(first.toList(), session.draw(3, "request-1").toList())
            val error = assertFailsWith<SortitionException> { session.draw(4, "request-1") }
            assertEquals(SortitionException.CONFLICT, error.code())
        }
    }

    @Test
    fun `stages are records`() {
        val plan = listOf(Stage("second", 2), Stage("first", 1))
        val results = Lottery.drawStages(Items.of("a", "b", "c", "d"), plan, 5L)
        assertEquals(listOf(2, 1), results.map { it.winners.size })
        assertEquals(listOf("second", "first"), results.map { it.name })
    }
}
