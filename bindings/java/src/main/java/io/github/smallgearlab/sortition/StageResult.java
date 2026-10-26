package io.github.smallgearlab.sortition;

import java.util.List;

/**
 * The winners of one prize stage.
 *
 * @param name    the name of the prize
 * @param count   how many winners the stage asked for
 * @param winners the winners, in the order drawn
 */
public record StageResult(String name, int count, List<Pick> winners) {}
