package io.github.smallgearlab.sortition;

/**
 * One prize stage of a plan.
 *
 * @param name  the name of the prize
 * @param count how many winners
 */
public record Stage(String name, int count) {}
