import { describe, it, expect } from 'vitest';
import { looks_like_a_web_page, NOT_THE_DEVICE_MESSAGE } from './utils.svelte';

/**
 * The device never answers an API request with a web page, so one arriving
 * means the request did not reach it. Reported as an unparseable token, that
 * is true and completely useless to somebody standing in front of a device
 * wondering why the page is empty.
 */
describe('looks_like_a_web_page', () => {
    it('recognises a page however it is announced', () => {
        expect(looks_like_a_web_page('<!doctype html>\n<html lang="en">')).toBe(true);
        expect(looks_like_a_web_page('<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01//EN">')).toBe(
            true
        );
        expect(looks_like_a_web_page('<html><head><title>Sign in</title>')).toBe(true);
    });

    it('is not fooled by whitespace in front of it', () => {
        expect(looks_like_a_web_page('\n\n  <!doctype html>')).toBe(true);
    });

    /**
     * Real responses must not be mistaken for pages. A log line can contain
     * angle brackets, and a JSON body can contain HTML inside a string.
     */
    it('leaves real responses alone', () => {
        expect(looks_like_a_web_page('{"entries":[]}')).toBe(false);
        expect(looks_like_a_web_page('{"note":"<!doctype html> in a string"}')).toBe(false);
        expect(looks_like_a_web_page('[2026-08-31 INFO] parsed <Message> ok')).toBe(false);
        expect(looks_like_a_web_page('')).toBe(false);
        expect(looks_like_a_web_page('R A Y H U N T E R')).toBe(false);
    });
});

describe('NOT_THE_DEVICE_MESSAGE', () => {
    it('says what went wrong and what to try', () => {
        expect(NOT_THE_DEVICE_MESSAGE).toMatch(/never reached the device/);
        expect(NOT_THE_DEVICE_MESSAGE).toMatch(/mobile data/);
        expect(NOT_THE_DEVICE_MESSAGE).toMatch(/VPN/);
    });
});
