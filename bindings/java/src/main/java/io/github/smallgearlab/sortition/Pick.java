package io.github.smallgearlab.sortition;

/**
 * A winner.
 *
 * @param index the position in the list that was passed
 * @param id    the candidate's ID: 32 bytes of SHA-256 as 64 lowercase hex characters
 */
public record Pick(int index, String id) {}
