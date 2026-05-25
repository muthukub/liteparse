from .parser import LiteParse, search_items
from .types import (
    LiteParseConfig,
    ParseResult,
    ParsedPage,
    TextItem,
    ScreenshotResult,
    ParseError,
)

__version__ = "2.0.0"
__all__ = [
    "LiteParse",
    "LiteParseConfig",
    "ParseResult",
    "ParsedPage",
    "TextItem",
    "ScreenshotResult",
    "ParseError",
    "search_items",
]
